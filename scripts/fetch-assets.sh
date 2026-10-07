#!/bin/zsh
# Downloads the Black Ops III Mod Tools files this project reads (the AP9 kit, the arms and
# their textures: assets/filelists/*.txt, about 1.45 GB) from your own Steam account, into
# BO3_MODTOOLS (default ~/Developer/game-assets/bo3-modtools), outside this repository.
# You need Black Ops III on Steam with access to the Mod Tools (app 455130). You sign in by
# scanning a QR code with the Steam app; nothing is stored by this script.
#   DEPOTDOWNLOADER=/path/to/DepotDownloader scripts/fetch-assets.sh
set -euo pipefail
ROOT=${0:A:h:h}
DD=${DEPOTDOWNLOADER:-$(command -v DepotDownloader || true)}
[[ -x "$DD" ]] || { echo "DepotDownloader not found: get it from github.com/SteamRE/DepotDownloader and set DEPOTDOWNLOADER." >&2; exit 1; }
DEST=${BO3_MODTOOLS:-$HOME/Developer/game-assets/bo3-modtools}
LIST=$(mktemp)
cat "$ROOT"/assets/filelists/*.txt > "$LIST"
mkdir -p "$DEST"
"$DD" -app 455130 -depot 455131 -filelist "$LIST" -qr -dir "$DEST"
rm -f "$LIST"
echo "Mod Tools files in $DEST"
