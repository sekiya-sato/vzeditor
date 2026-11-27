#!/usr/bin/env bash
# Ubuntu WSL2 のローカル配布先を更新する。
set -euo pipefail

repo=$(cd -- "$(dirname -- "$0")/.." && pwd)
cd -- "$repo"
target_dir="${CARGO_TARGET_DIR:-$HOME/vz-target}"
cargo test --locked --target-dir "$target_dir"
# 古い Ubuntu（22.04 以降）でも動くよう、glibc 2.35 向けにリンクする。
glibc_target=x86_64-unknown-linux-gnu.2.35
if ! command -v cargo-zigbuild >/dev/null; then
    printf 'cargo-zigbuild がありません。uv tool install cargo-zigbuild で入れてください\n' >&2
    exit 1
fi
if ! command -v zig >/dev/null && [[ -z ${CARGO_ZIGBUILD_ZIG_PATH:-} ]]; then
    # uv tool install cargo-zigbuild が入れた ziglang の zig を使う。
    zig_bin=$(find "$HOME/.local/share/uv/tools/cargo-zigbuild" -path '*/ziglang/zig' -type f -print -quit 2>/dev/null || true)
    if [[ -z $zig_bin ]]; then
        printf 'zig がありません\n' >&2
        exit 1
    fi
    export CARGO_ZIGBUILD_ZIG_PATH=$zig_bin
fi
cargo zigbuild --locked --release --bin vz --target "$glibc_target" --target-dir "$target_dir"

install_dir="$HOME/bin"
dest="$install_dir/vz"
source="$target_dir/x86_64-unknown-linux-gnu/release/vz"
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
