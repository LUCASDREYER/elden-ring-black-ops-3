#!/bin/sh
# Refuses game files and large binaries. The pre-commit hook runs it on staged files; CI runs
# it on every tracked file (`all`). This repository never carries game content: see README.
set -eu
cd "$(git rev-parse --show-toplevel)"
if [ "${1:-staged}" = all ]; then
  list=$(git ls-files)
else
  list=$(git diff --cached --name-only --diff-filter=ACMR)
fi
bad=0
refuse() { echo "refused ($2): $1"; bad=1; }
printf '%s\n' "$list" | while IFS= read -r f; do
  [ -n "$f" ] || continue
  lower=$(printf '%s' "$f" | tr '[:upper:]' '[:lower:]')
  case "$lower" in
    *.tif|*.tiff|*.dds|*.iwi|*.tga|*.exr|*.hdr) refuse "$f" "texture" ;;
    *.xmodel_bin|*.xanim_bin|*.xmodel_export|*.xanim_export|*.semodel|*.seanim|*.cast) refuse "$f" "model or animation" ;;
    *.gdt|*.gdf|*.csv.bin) refuse "$f" "asset definition" ;;
    *.ff|*.fd|*.xpak|*.sabs|*.sabl|*.pak|*.dcx|*.bdt|*.bhd) refuse "$f" "game archive" ;;
    *.wav|*.flac|*.ogg|*.mp3|*.wem|*.bnk) refuse "$f" "audio" ;;
    *.sl2|*.sl2.bak|*.sav) refuse "$f" "save" ;;
    *.dll|*.exe|*.dylib|*.so|*.shm) refuse "$f" "binary" ;;
  esac
  case "$lower" in
    *model_export/*|*xanim_export/*|*source_data/*|*texture_assets/*|*game-assets/*|*sound_assets/*) refuse "$f" "game asset path" ;;
  esac
  if [ -f "$f" ] && [ "$(wc -c < "$f")" -gt 2000000 ]; then refuse "$f" "over 2 MB"; fi
  [ "$bad" -eq 0 ] || echo "x" >> "${TMPDIR:-/tmp}/check-no-game-files.$$"
done
if [ -f "${TMPDIR:-/tmp}/check-no-game-files.$$" ]; then
  rm -f "${TMPDIR:-/tmp}/check-no-game-files.$$"
  echo "This repository never carries game files (README, Legal). Remove them from the commit."
  exit 1
fi
echo "check-no-game-files: ok"
