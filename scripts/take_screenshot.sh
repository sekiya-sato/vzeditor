#!/usr/bin/env bash
# Helper script to capture screenshots for README.md
# Usage:
#   ./scripts/take_screenshot.sh editor   -> docs/screenshots/editor.png
#   ./scripts/take_screenshot.sh filer    -> docs/screenshots/filer.png

set -euo pipefail

TARGET="${1:-editor}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUT_DIR="${SCRIPT_DIR}/../docs/screenshots"
mkdir -p "${OUT_DIR}"
OUT_FILE="${OUT_DIR}/${TARGET}.png"

echo "3秒後にアクティブウィンドウを撮影します -> ${OUT_FILE}"
echo "VZ Editor のウィンドウを前面にしてお待ちください..."
sleep 3

# 1. GNOME Shell D-Bus screenshot (works cleanly on GNOME Wayland & X11)
if command -v gdbus >/dev/null 2>&1; then
    if gdbus call --session \
        --dest org.gnome.Shell.Screenshot \
        --object-path /org/gnome/Shell/Screenshot \
        --method org.gnome.Shell.Screenshot.ScreenshotWindow \
        true false false "${OUT_FILE}" >/dev/null 2>&1; then
        echo "保存しました (GNOME Screenshot): ${OUT_FILE}"
        exit 0
    fi
fi

# 2. Wayland (wlroots / labwc / sway) via grim
if [ -n "${WAYLAND_DISPLAY:-}" ] && command -v grim >/dev/null 2>&1; then
    grim "${OUT_FILE}"
    echo "保存しました (grim): ${OUT_FILE}"
    exit 0
fi

# 3. X11 via scrot
if command -v scrot >/dev/null 2>&1; then
    scrot -u "${OUT_FILE}"
    echo "保存しました (scrot): ${OUT_FILE}"
    exit 0
fi

echo "エラー: 自動撮影に失敗しました。GNOME の Print キーから撮影し、${OUT_FILE} に保存してください。" >&2
exit 1
