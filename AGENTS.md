# Guide for coding agents

Read `README.md`, then `CONTEXT.md` (where things stand and why), then `PLAN.md`.

## Rules

- **Never add game files.** No models, textures, animations, sounds, `.gdt` weapon files,
  fastfiles, saves, or renders of game assets. The program reads the player's own files at
  runtime (`BO3_MODTOOLS`, default `~/Developer/game-assets/bo3-modtools`). The pre-commit
  hook runs `scripts/check-no-game-files.sh`; run it yourself before any commit, and never
  bypass it. Keep snapshots and logs in a scratch folder outside the repo.
- **Offline only.** Elden Ring is only ever started through the bridge launcher
  (`scripts/play.sh`), which runs it without Easy Anti-Cheat and can't go online. Never start
  it modded any other way, never suggest playing online with the bridge installed.
- **No decompiled or leaked game code.** Formats come from open community tooling (PyCoD) and
  the player's own files.
- **Facts need evidence.** Measure, log, or render before claiming; record what was proven
  in `CONTEXT.md` or `artifacts/`.

## Build and check

```bash
cargo build --release
```

```bash
cargo test --release
```

`target/release/snapshot <dir>` renders the view model and HUD to PNG through the same
renderer the game uses, for checking by eye without the game or screen access.

## While it runs

- The guest prints `guest: perf ...` once a second; the bridge logs to
  `/tmp/ermc/er-bridge.log` (`perf:` and `tick breakdown` lines).
- Commands to the running guest: write a line to `/tmp/ermc/guest.cmd`: `max_ammo`,
  `points N`, `add_points N`, `round N`.
- `GUEST_NO_PREVIEW`, `GUEST_NO_HUD`, `GUEST_NO_HAND` switch parts off for measuring.
- Moving the player across Elden Ring zones by writing positions is unsafe (positions are per
  zone); use the game's own travel instead.
