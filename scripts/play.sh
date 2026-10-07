#!/bin/zsh
# Starts Elden Ring through the bridge (unless it's already running), then the guest.
#
#   scripts/play.sh
#
# Steam must be running and signed in inside the CrossOver bottle. Load into the world;
# the guest covers Elden Ring with a transparent overlay and takes over. F8 (or Esc) hands
# control to Elden Ring, F8 in Elden Ring hands it back, Cmd+Q quits the guest. The bridge
# source lives in a separate clone (ER_BRIDGE_REPO).
set -euo pipefail
ROOT=${0:A:h:h}
BRIDGE=${ER_BRIDGE_REPO:-$HOME/Developer/minecraft-crossover-bridge}
export ER_BOTTLE=${ER_BOTTLE:-"elden ring"}

if ! pgrep -f 'eldenring.exe' >/dev/null; then
  mkdir -p /tmp/ermc
  "$BRIDGE/elden-ring/scripts/launch_er.sh" >/tmp/ermc/launch.log 2>&1 &
  echo "Starting Elden Ring with the bridge (log: /tmp/ermc/er-bridge.log). Load into the world."
fi
if pgrep -f 'target/release/guest' >/dev/null; then
  echo "A guest is already running; quit it first (Cmd+Q)." >&2
  exit 1
fi
cargo build --release --manifest-path "$ROOT/Cargo.toml" -p guest
exec "$ROOT/target/release/guest"
