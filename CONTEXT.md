# Context

How this project is being made: where it stands, how to run it, what was decided and why,
and where the evidence is. PLAN.md is the plan; CHECKS.md settled its open questions. This
file is updated at the end of each working session.

## Where it stands (2026-10-07)

| Phase | State |
|---|---|
| Checks | Done. See CHECKS.md |
| 0: run the existing bridge | The bridge runs on this Mac (Elden Ring 1.17.1 in CrossOver, about 50 fps). The Minecraft mod built but wasn't run: our own guest proved the same thing |
| 1: Rust guest hello world | **Done, gate passed.** It drives Elden Ring's camera and composites a textured cube with depth into Elden Ring's frame, every frame in sync. You confirmed by eye that the cube hides behind walls (default depth buffer 0, no F3 needed) |
| 2: AP9 loader | **Done, gate passed.** `cod-assets` reads `XMODEL_BIN`, `XANIM_BIN` and `.tif`; the AP9, its magazine and the arms are GPU-skinned and animated into Elden Ring's hand layer. You saw it in game and fired and reloaded (4 reloads in the log). Evidence: artifacts/phase-2 |
| 3: gunplay | **Core working, confirmed in game.** You damaged enemies and walls stopped you. Log: 26 hits on `c4690` (2,596 max HP) at 77 HP each; two hits on you came back as 111 and 147 Elden Ring HP, health 100 -> 79 -> 51. Since then: ADS with BO3's own sight picture, sprint animations, muzzle flash, and the AP9's stats read from its BO3 weapon file. Those are built and checked offline, not yet tried in game |

## Running it

1. Steam running and signed in inside the CrossOver bottle `elden ring`.
2. `scripts/play.sh`: starts Elden Ring through the bridge if needed, then the guest.
3. Load into the world. The guest covers Elden Ring's game area with a transparent overlay
   that holds the keyboard and mouse (cursor locked and hidden): no window to click into.

| Key | Does |
|---|---|
| mouse | look |
| left click, R | fire (hold for automatic), reload |
| right click | aim down the sights |
| W A S D, Shift, Space | walk, sprint, jump |
| F | Elden Ring's action: open, pick up, rest at grace |
| **F8 or Esc** | **hand control to Elden Ring** (its camera, character, menus) |
| **F8 in Elden Ring** | **back to BO3** |
| V | walk / fly (noclip; the Tarnished reappears) |
| E / Q | up / down while flying |
| C | put the cube on the ground 3 m ahead |
| F5 | give the camera back to Elden Ring and start again from its view |
| Tab | release / recapture the mouse |
| F1 / F2 / F3 | depth test off, compositor test pattern, next occlusion depth buffer |
| Cmd+Q | quit the guest (Esc never quits) |

On a Mac keyboard, F-keys need fn unless "Use F1, F2, etc. keys as standard function keys"
is on (System Settings, Keyboard). The overlay gets out of the way by itself when Elden Ring
sits on its own screens (title, loading) for more than 3 s.

Health checks while it runs (from the bridge clone):

```bash
python3 ~/Developer/minecraft-crossover-bridge/elden-ring/scripts/erctl.py fps
```

```bash
grep -E "compositor:|sync:" /tmp/ermc/er-bridge.log | tail
```

## Layout

```
crates/bridge-link/   the shared-memory protocol: state, control, frames.shm, rays, entities,
                      damage queue, hit events, life counters, action prompt
crates/cod-assets/    readers for BO3 mod tools XMODEL_BIN, XANIM_BIN and .tif (format from
                      PyCoD, MIT); examples/inspect.rs and poses.rs dump files
crates/guest/         the guest: camera, walker, combat, HUD, view model, renderer, main loop;
                      bin/snapshot.rs renders the view model to PNG without the game
spikes/frame-readback/  the readback benchmark from CHECKS.md item 3
scripts/play.sh       start everything
artifacts/phase-N/    evidence per phase
```

The bridge itself is not vendored. It's a separate clone at
`~/Developer/minecraft-crossover-bridge`, based on `d1723873ec`, built and installed into
the game folder by its own `install_er_bridge.sh` (with `ER_BOTTLE="elden ring"`). Remove it
with `install_er_bridge.sh --remove`.

**The clone carries one local patch** (uncommitted, branch `perf-instrumentation`):

- `memutil.cpp`: `mem_readable` caches the regions `VirtualQuery` confirmed, for one game
  tick (`mem_cache_reset()` at the start of `task_tick`). Under Wine and Rosetta each query is
  a slow system call, and publishing entities made thousands a tick.
- `game.cpp`: a `tick breakdown (ms)` log line every 300 ticks, per section.

Measured in the Stranded Graveyard, 1280 x 720, Low: entity publishing 8.42 to 8.70 ms a tick
-> 0.10 ms; the whole tick 9 ms -> 0.33 ms; Elden Ring 25 -> 36 fps without the guest, 24 ->
34 to 36 fps with it. Hot-swapped with `reload_core.sh` (which also installs it for the next
launch). Worth offering upstream.

## Decisions

- **Phase 1 uses wgpu and winit directly, not Bevy.** The guest needs exact control of the
  depth convention and the readback, and nothing else yet. A standard left-handed `[0, 1]`
  projection gives the same numbers as OpenGL window depth, which the compositor expects,
  so there's no conversion pass (tested in `guest`). Bevy renders reverse-Z with an
  infinite far plane, which would need one. Revisit when Phase 2 brings skeletal animation
  and assets, where an engine starts paying for itself.
- **Synchronous readback, one frame per Elden Ring frame.** The guest renders only when
  Elden Ring's heartbeat advances, reads back, writes the slot, then publishes the pose,
  in that order, exactly like the Minecraft mod. Measured: every presented frame had the
  exact pose (`sync: exact 300, older 0, missing 0`). The pipelined readback from the spike
  is there if frame time ever becomes the limit.
- **The Tarnished stays visible** and the camera flies free. Standing in for the player
  (`MOVE_HUNTER`, `HIDE_HUNTER`) comes with Phase 3.
- **Cube placement uses the bridge's raycast** against Elden Ring's collision. That's the
  same ray block Phase 3 hitscan will use, so it's already exercised.

- **BO3 assets are read at runtime** from `~/Developer/game-assets/bo3-modtools` (or
  `BO3_MODTOOLS`): the AP9 kit, 455 files, 1.43 GB, fetched with DepotDownloader's
  `-filelist` (CHECKS.md item 2). Nothing from it is in this folder.
- **The hand layer uses its own projection** (`HAND_FOV_DEG` 55, vertical) in eye space:
  inches, X forward, Y left, Z up, mirrored into Elden Ring's left-handed view.
- **Rays are batched once per frame,** the walker's 17 probes plus one ray per bullet, and
  answered by the next game frame. Characters are tested against their published hitboxes at
  fire time; the world ray decides if a wall was nearer.
- **Weapon stats come from BO3's own definition:** `source_data/smg_standard.gdt`, entry
  `smg_standard_mp`, the mod tools' sample SMG built on the AP9 (it names the AP9's
  animations). Read at startup by `stats.rs` with a small `.gdt` reader in `cod-assets`:
  fire time 0.0833 s, 30 rounds, damage 35/32/29/23/23 by range tiers (inches in the file),
  head x1.1, hip spread 2 to 5 degrees (+0.3 a shot, +5 moving, -4 a second), ADS 0.4,
  ADS in/out 0.25 s, reload counts at 1.8 s (2.3 s empty). Built-in copies if the file is
  missing.
- **BO3 damage to bridge points: x 3/35.** The close-range 35 is 3 points; the bridge scales
  by the square root of max HP, so a basic soldier takes about a dozen bullets and a
  2,600-HP knight about 35 (measured: 77 HP a bullet).
- **ADS follows BO3's rig:** its ADS clip moves only `tag_torso`, which carries arms and gun;
  that travel (-3.61, 3.42, 0.75 in) is the ADS shift. The sight line then points 0.67 deg low,
  so the rig is pitched up 0.0118 rad about the eye at full ADS: the tritium dot measures at
  (639.4, 359.8) for a 1280 x 720 centre of (640, 360) in `bin/snapshot`. `fire_ads` plays
  while aimed; both fields of view narrow to 0.82.
- **Health is the guest's:** the stand-in's hits come back as `HunterEvents`, scaled by the
  Tarnished's max HP to a 100-point bar with CoD regeneration. At 0 the guest bumps
  `mcDeaths`, the bridge kills the Tarnished, Elden Ring plays its death and respawn, and
  `hostLife` tells the guest where to pick up.

## Zombies mode

The HUD and rules follow Black Ops 3 Zombies (`zombies.rs`, `hud.rs`): start with 500
points, +10 a hit, +50 a kill, +100 a headshot kill (credited when a character we hit dies
within 3 s); solo rounds of 6, 8, 13, 18, 24 kills then `0.09r^2 - 0.0029r + 23.958`; four
magazines in reserve; a 3% Max Ammo per kill. The round counter is red chalk tallies (the
fifth strikes through four) then hand-drawn numerals, flashing white on a new round; the
score sits bottom right with "+10" pop-ups; no health bar, red edges instead.

**The artwork is drawn by the guest:** the mod tools have no zombies HUD images (`zm_hud`
holds only two Death Machine icons) and **no zombies character arms** (only the multiplayer
"warrior" viewhands; Dempsey has a single glove specular texture). The Primis crew's arms and
the real HUD live in BO3's own fastfiles, which would mean installing BO3 (about 32 GB, in the
bottle) and extracting with Windows-only tools.

## Sound and view-model motion

- **Sound (`audio.rs`) is synthesised at startup,** not BO3's recordings: the mod tools ship
  no weapon audio (one test WAV); the AP9 names the Kuda's aliases (`wpn_smg_standard_fire_plr`,
  `wpn_smg_plr_decay`, `wpn_generic_dryfire_plr`), which live in BO3's own sound banks. Five
  shot variants (crack, 9 mm body, thump, bolt, short room), a decay when a burst ends, dry
  fire, reload sounds on the animations' notetracks (`clip_out`, `clip_in`, `rechamber pull
  back`, `rechamber release`), ADS cloth, a hit tick, a round-change chord and a Max Ammo
  arpeggio. Mixed in software on a `cpal` stream with soft clipping.
- **Motion (`motion.rs`) is what BO3 does in its engine:** a walking bob in step with the
  ground covered (1.5 m a cycle), breathing at rest, look sway from the weapon file
  (`swayLerpSpeed 6`, `swayMaxAngle 5`, `swayYawScale -2`, `swayPitchScale -1.5`, the ADS
  versions, `adsViewBobMult 0.3`), and a spring dip on landing. One eye-space transform on top
  of the animation (`Viewmodel::set_rig`).

## Known issues

- Bridge log: `combat: AtkParam_Pc 10176000 not found; no hit reactions`. Hits will still
  apply but enemies won't stagger. That matters for Phase 3.
- The arms (multiplayer "warrior" viewhands) use their real textures (11 files, 19.2 MB,
  `arms-filelist.txt`), tinted olive drab and brown leather in `outfit_tint`: BO3 outfits are
  near-white textures the game tints per customisation. 5 of 7 materials are textured; `hand_b`
  isn't in the depot and the wrist tablet has no colour map. A material whose named file is
  missing falls back to one named after the material (`arm_lt_a1` -> `arm_lt_a_c.tif`).
- No tracers, impact effects or sound yet (the mod tools have no AP9 sounds). The muzzle
  flash is procedural, not BO3's effect.
- **Performance, measured** (Stranded Graveyard): the guest costs about 1 fps; its own work is
  3.6 ms a frame (render and readback 1.3, publish 1.7, preview 0.5). The guest prints a
  `guest: perf` line every second; `GUEST_NO_PREVIEW`, `GUEST_NO_HUD`, `GUEST_NO_HAND` switch
  parts off for measuring. Frames of 60 to 150 ms still happen with or without the guest:
  Elden Ring's own (streaming, or D3DMetal), not investigated yet.
- **Lifts:** the terrain ray filter (`0x5D`) doesn't see lift platforms, so a lift drops the
  walker out of the world. The walker now returns to the last confirmed ground after 2.5 s or
  25 m of falling. Riding lifts needs a filter that sees them (to test at a lift).
- The guest sets `NO_RELIGHT`: the bridge's relighting tints pixels with a blur of the scene
  behind them, which made the gun look see-through.
- If Elden Ring stops presenting for 2 s (closed, crashed), the overlay hides and frees the
  mouse; it comes back when frames resume.
- Only one guest may run. Two fight over the camera; `scripts/play.sh` refuses a second.

## Rules (from PLAN.md)

- No game files in this folder, ever. Assets are read at runtime from the player's own
  installs or downloads; the AP9 download goes outside the repo.
- Offline only. The bridge launcher starts Elden Ring without Easy Anti-Cheat. Start the
  game only through it, never through Steam's Play button (which would also install
  pending patches; auto-update is set to wait for launch).
- Back up your saves before playing: the `EldenRing` folder under the bottle's
  `drive_c/users/*/AppData/Roaming/`. Steam Cloud syncs whatever a modded session writes.
