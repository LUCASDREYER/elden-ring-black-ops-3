# Elden Ring × Black Ops 3

A fan experiment: Black Ops 3's AP9 in first person inside Elden Ring, with a Black Ops 3
Zombies-style HUD, on an Apple Silicon Mac. Elden Ring runs as the real game under CrossOver;
a Rust program draws the gun, the arms and the HUD into Elden Ring's own frame, drives its
camera, and shoots its enemies through
[justbustin/minecraft-crossover-bridge](https://github.com/justbustin/minecraft-crossover-bridge),
an open-source DLL that turns Elden Ring into a host for other programs.

Built with [Claude Code](https://claude.com/claude-code), step by step, with the evidence for
each step kept in the repo. It's shared for other people (and agents) building game mashups.

**Not affiliated with, endorsed by or connected to Activision, Treyarch, Bandai Namco or
FromSoftware.** This repository contains **no game files**. You need your own copies of both
games, and the program reads them from your disk at runtime.

## Read this first: staying safe

The bridge starts Elden Ring **offline, without Easy Anti-Cheat**. Mods that run online are
what gets accounts banned. To stay clear of that:

- **Only ever play modded through the bridge launcher** (`scripts/play.sh`). It cannot reach
  Elden Ring's online servers.
- **Never take the bridge online.** Before playing online again, remove it from the game
  folder: `elden-ring/scripts/install_er_bridge.sh --remove` in the bridge clone. A normal
  Steam launch doesn't load it, but removing it leaves nothing to question.
- **Use a separate character, and back up your saves first:** the `EldenRing` folder under
  the CrossOver bottle's `drive_c/users/*/AppData/Roaming/`. Steam Cloud syncs whatever a
  modded session writes, so restore your backup before going online with that profile.
- **Set Elden Ring to update only when launched** (Steam, Properties, Updates, "Wait until I
  launch the game"). The bridge supports exactly one game version, App Ver. 1.17.1.

## Legal

- **No game files, ever.** No models, textures, animations, sounds, weapon data files or
  saves are committed. A pre-commit hook and a GitHub check (`scripts/check-no-game-files.sh`)
  refuse them. `assets/filelists/` holds only the *names* of the files to fetch from your own
  Steam account.
- **Bring your own copies:** Elden Ring (Steam, PC) and Call of Duty: Black Ops III (Steam),
  whose owners can download the Black Ops III Mod Tools.
- **Activision's Mod Tools EULA** licenses the tools' content for use with Black Ops III. Loading
  it into another game, as this project does on your machine, goes beyond that. Whether to do
  so with your own copy is your call; this repository only contains code that reads files you
  already have, and it's never sold.
- **Non-commercial.** No sales, paywalls or donations tied to it.
- The model and animation formats were implemented from
  [PyCoD](https://github.com/CoDEmanX/blender-cod) (MIT), not from game code. Nothing here
  comes from decompiled or leaked sources of either game. See `THIRD_PARTY_NOTICES.md`.

## What works

- First person: walk on Elden Ring's ground (its own collision, through raycasts), sprint,
  jump. The Tarnished is hidden at your feet as a stand-in, so enemies attack "you".
- The AP9 with arms, animated from BO3's view-model animations: raise, idle, fire, aim down
  the sights, reload, sprint. Stats from the AP9's own weapon file (fire rate, damage by range,
  spread, ADS timing).
- Hitscan into Elden Ring's enemies, damage through the bridge, one shared life.
- Zombies HUD and rules: chalk round counter, points, Max Ammo. Synthesised gun sounds,
  view-model bob and sway.
- A transparent overlay holds the mouse and keyboard; F8 switches between BO3 and normal
  Elden Ring.

Not yet: real BO3 audio and zombies character arms (neither is in the Mod Tools), lifts (you
fall through them, then get put back), enemy stagger.

## Requirements

- Apple Silicon Mac, macOS 15.4 or later, about 65 GB free
- [CrossOver](https://www.codeweavers.com/crossover) 26 (paid, with a trial), a bottle with
  Windows Steam and Elden Ring (App Ver. 1.17.1), graphics set to D3DMetal
- Black Ops III on Steam, with access to the Mod Tools (Steam app 455130)
- Rust, Homebrew `mingw-w64` (to build the bridge), and
  [DepotDownloader](https://github.com/SteamRE/DepotDownloader) for the partial download

## Setup

1. **The bridge:** clone `justbustin/minecraft-crossover-bridge` to
   `~/Developer/minecraft-crossover-bridge` (or set `ER_BRIDGE_REPO`), check out
   `d1723873ec`, and follow its `elden-ring/README.md` to install it into your bottle (set
   `ER_BOTTLE` if your bottle isn't named `Elden Ring`). Optional, but it lifts the frame rate a
   lot: `git apply` `patches/bridge-perf.patch` first (see the patch's header).
2. **Your BO3 files (1.45 GB of the 28 GB Mod Tools):**
   `scripts/fetch-assets.sh` downloads exactly the files in `assets/filelists/` from your
   own account (you sign in with a Steam QR code) into `~/Developer/game-assets/bo3-modtools`
   (or `BO3_MODTOOLS`).
3. **Play:** start Steam in the bottle, then `scripts/play.sh` and load into the world.

## Controls

| Key | Does |
|---|---|
| mouse, left click, right click, R | look, fire, aim down the sights, reload |
| W A S D, Shift, Space | walk, sprint, jump |
| F | Elden Ring's action (doors, items, graces) |
| F8 or Esc | hand control to Elden Ring; F8 in Elden Ring comes back |
| V | walk / fly |
| Cmd+Q | quit the guest |

On a Mac keyboard, F-keys need fn unless they're set as standard function keys.

## How it's built

- `crates/bridge-link`: the bridge's shared-memory protocol in Rust.
- `crates/cod-assets`: readers for BO3 Mod Tools `XMODEL_BIN`, `XANIM_BIN`, `.tif` and `.gdt`.
- `crates/guest`: the program: wgpu rendering into the bridge's layers, the walker, combat,
  view model, zombies rules, HUD, audio. `bin/snapshot` renders the weapon to PNG without the
  game.
- `PLAN.md` (the plan), `CHECKS.md` (its open questions, answered), `CONTEXT.md` (where things
  stand and why), `artifacts/` (evidence per phase). `AGENTS.md` is the short guide for coding
  agents.

## Licence

MIT, see `LICENSE`. Third-party notices in `THIRD_PARTY_NOTICES.md`.
