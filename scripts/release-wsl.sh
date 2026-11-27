#!/usr/bin/env bash
# Ubuntu WSL2 のローカル配布先を更新する。
set -euo pipefail

repo=$(cd -- "$(dirname -- "$0")/.." && pwd)
cd -- "$repo"
target_dir="${CARGO_TARGET_DIR:-$HOME/vz-target}"
cargo test --locked --target-dir "$target_dir"
cargo build --locked --release --bin vz --target-dir "$target_dir"

install_dir="$HOME/bin"
dest="$install_dir/vz"
source="$target_dir/release/vz"
mkdir -p -- "$install_dir"
exec 9>"$install_dir/.vz-release.lock"
flock -n 9
pending=$(mktemp "$install_dir/.vz.XXXXXX")
trap 'rm -f -- "$pending"' EXIT
install -m 0755 -- "$source" "$pending"
cmp -- "$source" "$pending"

installed=$(readlink -f -- "$dest")
installed_pids() {
    local proc path
    for proc in /proc/[0-9]*; do
        path=$(readlink "$proc/exe" 2>/dev/null || true)
        if [[ $path == "$installed" || $path == "$installed (deleted)" ]]; then
            printf '%s\n' "${proc##*/}"
        fi
    done
}

proc_start() {
    awk '{ print $22 }' "/proc/$1/stat" 2>/dev/null || true
}

mapfile -t pids < <(installed_pids)
if (( ${#pids[@]} )); then
    id=$(tr -d '-' </proc/sys/kernel/random/uuid)
    printf 'リリース ID %s / 保存対象 %s プロセス\n' "$id" "${#pids[@]}"
    declare -A start_times=()
    for pid in "${pids[@]}"; do
        start_times[$pid]=$(proc_start "$pid")
    done
    "$pending" --release-save-and-quit "$id" "${pids[@]}"
    deadline=$((SECONDS + 60))
    for pid in "${pids[@]}"; do
        while [[ -n ${start_times[$pid]} && $(proc_start "$pid") == "${start_times[$pid]}" ]]; do
            if (( SECONDS >= deadline )); then
                printf 'PID %s が終了していません\n' "$pid" >&2
                exit 1
            fi
            sleep 0.2
        done
    done
fi
if [[ -n $(installed_pids) ]]; then
    printf '配置先から新しく起動された VZ があります\n' >&2
    exit 1
fi
mv -fT -- "$pending" "$dest"
cmp -- "$source" "$dest"
if (( ${#pids[@]} )); then
    # 復元した GUI に配置用ロックを継承させない。
    "$dest" --release-restore "$id" 9>&-
fi
printf '配置済み: %s\n' "$dest"
