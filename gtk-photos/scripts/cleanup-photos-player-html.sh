#!/bin/bash
# Remove leftover video/image viewer temp files from the configured Photos drive and /tmp.
# Safe to run anytime; current app versions store players only under /tmp.
#
# Cleans:
#   - .gtk-photos-player-*.html under photo_drive and /tmp
#   - /tmp/chromium-video-*  (Chromium --user-data-dir for video viewers)
#   - /tmp/chromium-image-*  (Chromium --user-data-dir for image viewers)
#   - /tmp/chromium-html-*   (temp HTML dirs for image viewers)
#
# Photos drive root is read from ~/.config/gtk-apps/gtk-photos/config.json (photo_drive).

set -euo pipefail

dry_run=0
if [[ "${1:-}" == "--dry-run" || "${1:-}" == "-n" ]]; then
    dry_run=1
fi

CONFIG_JSON="${XDG_CONFIG_HOME:-$HOME/.config}/gtk-apps/gtk-photos/config.json"

photo_drive=""
if [[ -f "$CONFIG_JSON" ]]; then
    photo_drive=$(python3 -c "
import json, sys
try:
    d = json.load(open(sys.argv[1], encoding='utf-8'))
    print(d.get('photo_drive') or d.get('explore_directory') or '')
except Exception:
    print('')
" "$CONFIG_JSON")
fi

remove_player_html() {
    local root="$1"
    while IFS= read -r -d '' file; do
        if (( dry_run )); then
            echo "would remove: $file"
        else
            rm -v -- "$file"
        fi
    done < <(find "$root" -name '.gtk-photos-player-*.html' -type f -print0 2>/dev/null)
}

remove_dirs() {
    local pattern="$1"
    shopt -s nullglob
    for dir in $pattern; do
        if [[ -d "$dir" ]]; then
            if (( dry_run )); then
                echo "would remove dir: $dir"
            else
                rm -rf -- "$dir"
                echo "removed dir: $dir"
            fi
        fi
    done
    shopt -u nullglob
}

if [[ -n "$photo_drive" && -d "$photo_drive" ]]; then
    echo "Scanning Photos drive from config: $photo_drive"
    remove_player_html "$photo_drive"
else
    echo "No photo_drive configured (or path missing) in $CONFIG_JSON — skipping drive scan."
fi

tmp_players=/tmp/gtk-photos-players
if [[ -d "$tmp_players" ]]; then
    echo "Scanning $tmp_players"
    remove_player_html "$tmp_players"
    if [[ -z "$(ls -A "$tmp_players" 2>/dev/null || true)" ]]; then
        if (( dry_run )); then
            echo "would remove dir: $tmp_players"
        else
            rmdir -- "$tmp_players" 2>/dev/null || true
            echo "removed dir: $tmp_players"
        fi
    fi
fi

echo "Scanning /tmp Chromium viewer profiles:"
remove_dirs "/tmp/chromium-video-*"
remove_dirs "/tmp/chromium-image-*"
remove_dirs "/tmp/chromium-html-*"

if (( dry_run )); then
    echo "Dry run only — no files deleted."
fi
