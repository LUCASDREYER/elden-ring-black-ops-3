# Black Ops 3 inside Elden Ring, on this Mac

Handoff written 2026-10-07 at the end of a long research session. Everything marked
**verified** was checked that day by reading code, APIs or the Steam store. Everything
marked **to check** was not. Start a new Claude Code session in this folder and point it
at this file.

## The idea in one paragraph

Elden Ring runs as the real game under CrossOver. A small open-source DLL already exists
that turns Elden Ring into a host: it exposes the camera, the player, terrain ray queries,
the enemy list with hitboxes, a damage queue in both directions, doorways and a frame
compositor over shared memory. Today a Minecraft mod is the guest. The project is to write
a **Rust and Bevy guest instead**, whose content is **Black Ops 3's weapons**, loaded from
BO3's own asset files. First-person Call of Duty gunplay in the Lands Between, drawn into
Elden Ring's own frame, hitting Elden Ring's own enemies. The join is the bridge protocol;
the rewrite is a loader for BO3's model, animation and image formats.

Nobody has done this pairing. The closest listed projects are Mario 64 inside Elden Ring,
two Minecraft-in-Elden-Ring bridges, and Halo characters inside the MW2 rewrite.

## Verified facts

- **The Elden Ring bridge is guest-agnostic.** `justbustin/minecraft-crossover-bridge`
  (MIT, commit `d1723873ec`), file `elden-ring/er-bridge/include/bridge_protocol.h`. One
  8 MB file-backed mapping at `/tmp/ermc/bridge.shm`, little-endian, fixed offsets:
  header, game state (camera, player, window, matrices, death flags), control (override
  camera, hide the Tarnished, composite), a debug command mailbox, up to 8,192 rays per
  batch against the game's collision, an entity table of 256 with large/small monster
  types, a damage ring of 256 with critical and outward flags, 64 passages (open
  doorways), and a separate frames file with 3 slots of up to 1920 x 1200 RGBA plus depth
  and a **hand layer flag**. The word "Minecraft" appears only in comments and field
  names. The Java mirror of the offsets is `dev.ermc.bridge.link.Protocol`; the frame
  readback is `FramePassthrough.java`. Rewrite both in Rust and the DLL needs no change.
- **The bridge code is clean** at that commit: the three scripts read in full and 86
  source files scanned. No networking, no process launching, no registry writes, no
  prebuilt binaries except the Gradle wrapper. Not a line-by-line audit.
- **Elden Ring side requirements**: CrossOver 26 (paid, 14-day trial), bottle named
  exactly `Elden Ring`, graphics D3DMetal, macOS 15.4 or later (this Mac is 26.6.2),
  Steam PC version of the game, **App Ver. 1.17.1** only, launched offline without
  anti-cheat through the bridge's launcher. Full steps: the bridge's own
  `elden-ring/README.md`.
- **Black Ops 3 on Steam** (app 311210): Windows and Mac, R$ 329,90 full price on
  2026-10-07. The Mac port by Aspyr is on Steam and runs on Apple Silicon; the community
  patch `BO3MacFix` fixes its old bugs. The mod tools, HydraX and Greyhound are
  Windows-only.
- **BO3's mod-tool asset formats are documented in open code.** `xmodel_bin` and
  `xanim_bin` (LZ4-compressed structured binaries) are read and written by the Blender
  addon BetterBetterBlenderCOD and by Export2bin; the Modme wiki's weapon-porting guides
  describe the weapon asset set (view model, world model, animations, GDT entries).
- **The development Mac**: M1 Pro, 16 GB, 57 GB free on 2026-10-07, Rust 1.99. Nothing
  else from the stack installed yet.
- **The working method:** a `CONTEXT.md` journal and an evidence folder per phase, claims
  only with evidence, and a scrub before any public push.

## To check before writing any code

1. **Ownership and platform.** Elden Ring must be Steam PC. BO3 must be on Steam (the Mac
   and Windows licences are the same app).
2. **The mod tools depot.** The Steam app id believed to be the BO3 mod tools (455130)
   returned nothing from the store API on 2026-10-07. Find the real id with
   `steamcmd +app_info_print 311210` after logging in, confirm the mod tools are free for
   owners, confirm the depot includes the weapon view models and animations, and get its
   size. It is believed to be tens of GB, far more than this Mac can hold beside Elden
   Ring, so plan a **partial download** with DepotDownloader's file list, or pull the
   depot onto an external drive.
3. **Frame readback in Bevy.** The guest must copy its colour and depth into the shared
   frame slot every frame. Prove a wgpu readback at 60 fps on this Mac before anything
   else, since the whole compositing design depends on it.
4. **Disk.** Elden Ring 60 GB, CrossOver and bottle about 5 GB, BO3 assets unknown.
   57 GB free is not enough; something has to be uninstalled.

## Phases, each with a gate

| Phase | Work | Gate | Guess |
|---|---|---|---|
| 0 | Run the existing bridge with Minecraft on this Mac, following the setup guide | Steve stands in the Lands Between on this Mac | 1 week, mostly installs and downloads |
| 1 | Rust guest hello world: a Bevy app that maps the shm, reads state, drives the camera with WASD and mouse, composites one textured cube with depth into Elden Ring's frame | The cube hides correctly behind a wall | 1 to 2 weeks |
| 2 | BO3 loader: `xmodel_bin`, `xanim_bin` and `.tif` textures from the mod tools depot into Bevy meshes, skeletons and clips; the AP9 view model and arms animating in the hand layer. The AP9 is the only complete weapon in the mod tools (see CHECKS.md); a 1.43 GB partial download | The AP9 plays its idle, fire and reload in first person over Elden Ring | 2 to 3 weeks |
| 3 | Gunplay: hitscan through the ray block and the entity table, damage through the queue, the Tarnished stand-in taking hits, HUD, sounds, weapon stats from the game's data | A soldier at the first grace dies to a magazine; a boss takes dozens | 1 to 2 weeks |
| 4 | Release as bring-your-own-copies, source only, no assets; submit to `bailo167/awesome-game-mashups` | Someone else runs it | 1 week |

Treat the guesses as hypotheses. Keep an artifact folder per phase with the evidence, so a
future session can read what was proven.

## Rules that keep it alive

- **No game files in the repo, ever.** Not models, not sounds, not fastfiles. The code
  reads the player's own installs at runtime. Add a publish check that refuses them.
- **Offline only.** Elden Ring without anti-cheat, never online with the bridge installed.
  Back up saves; use a separate character.
- **No decompiled code.** Read formats from open community tooling and your own files.
  Do not vendor anything from the leaked or decompiled sources that exist for either game.
- **Document the process, not just the result.** A `CONTEXT.md` for this project from
  day one.

## Repos to read first, in order

1. `justbustin/minecraft-crossover-bridge`: `docs/how-it-works.md`, then
   `elden-ring/er-bridge/include/bridge_protocol.h`, then the Java `link/` and
   `client/FramePassthrough.java`.
2. `chasmlol/ELDEN-RING-Playermodel-Rust` (2026-10-06): Elden Ring movement and combat
   timings read from the game's files, in Bevy 0.19. Useful for Elden Ring file reading
   in Rust, not needed for the bridge.
3. BetterBetterBlenderCOD and Export2bin for the `xmodel_bin` / `xanim_bin` formats.
4. `bailo167/awesome-game-mashups` for prior work before each phase.

## First message for the new session

"Read PLAN.md. Do the four 'to check' items first, without installing anything until
you have told me what each install costs in time and disk. Then propose the Phase 1
repository layout."
