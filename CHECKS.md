# The four "to check" items from PLAN.md

Checked 2026-10-07 on this Mac (M1 Pro, 16 GB, macOS 26.6.2). Nothing was installed except
the Rust crates for the readback spike (97 MB in `~/.cargo/registry`, build output in a
temporary folder). Evidence is the local Steam data, the public SteamCMD app info at
`api.steamcmd.net`, the bridge source at commit `d1723873ec`, and the spike in
`spikes/frame-readback/`.

| # | Item | Result |
|---|---|---|
| 1 | Ownership and platform | **Done.** Both owned on Steam. Elden Ring installed in CrossOver at exactly 1.17.1 |
| 2 | Mod tools depot | **Done.** One complete first-person weapon (AP9 SMG) plus arms. The MR6 and Kuda are not there |
| 3 | Frame readback at 60 fps | **Passes standalone**: 287 fps at 1920 x 1200 with all four layers |
| 4 | Disk | 45 GB free after Elden Ring. Enough for the rest of Phase 0, tight with the full mod tools |

## 1. Ownership and platform

- **Black Ops 3 (311210): owned on Steam.** Steam's local config for the account records it
  as played.
- **Mod tools (455130): in the library** alongside BO3 (Steam had cached its library art).
- **Elden Ring (1245620): owned and installed** (update, 2026-10-07 04:20). It's in a CrossOver
  bottle, downloaded through that bottle's Steam, so it's the PC licence:
  - `eldenring.exe` file version **2.7.1.0**, which is App Ver. 1.17.1, the bridge's version.
    Steam build 25080141, fully installed (`StateFlags` 4), 56.1 GB on disk. Depots 1245621,
    1245624 and 1896300; Shadow of the Erdtree is not installed.
  - Bottle `elden ring` (lowercase): template `win10_64`, graphics `d3dmetal`, MSync on.
    CrossOver 26 is in `~/Applications`, which the scripts check.
  - The game has run once: it wrote `GraphicsConfig.xml` and a save, `ER0000.sl2`.
    **Backed up** outside the repo, with checksums matching the original.
  - **Auto-update set to "Wait until I launch the game"** (`AutoUpdateBehavior` 1, confirmed
    in the manifest at 04:28). Start the game only through the bridge launcher. Steam's
    Play button would install any waiting patch.
  - The bridge scripts default to the bottle name `Elden Ring`. This disk is
    case-insensitive APFS, so the paths resolve, but CrossOver's own `--bottle` lookup is
    untested with a different case. Run the scripts with `ER_BOTTLE="elden ring"` to be safe.

## 2. The mod tools depot

From `api.steamcmd.net/v1/info/455130` and `/499270`:

| Depot | What | Download | On disk |
|---|---|---|---|
| 455131 | Mod tools, Windows | 24.86 GB | 28.15 GB |
| 499270 | Optional DLC "BOIII Mod Tools - Additional Assets" (requires 455130) | 98.46 GB | 104.52 GB |

- The plan's id was right. The store API returns nothing for this "Tool" type app, but the
  SteamCMD app info does.
- Public descriptions say the Additional Assets are extra level-building FX, entities and
  map models from the official maps. Skip them.
- Whether it's free for owners isn't shown in the app info, but item 1 suggests it's already
  in your library. Check under Library, Tools.
### What's in it (file list read 2026-10-07)

Listed with DepotDownloader 3.4.0 (`-manifest-only`, your Steam login via QR). Manifest
`7421982922197022459`, dated 2020-07-13: 187,611 files, 28.15 GB. The file list is in
`~/Developer/tools/bo3-modtools-manifest/`, outside this repo; the file lists made from it are
in `assets/filelists/`.

- **Formats:** 4,255 `xmodel_bin` (0.29 GB), 980 `xanim_bin` (0.36 GB) and 7,549 `.tif`
  textures (23.3 GB). These are all plain files, so no Windows tool is needed to read them.
- **One complete first-person weapon: the AP9 SMG** (`model_export/weapons_t7/ap9_gremlin/`):
  - view model `wpn_t7_smg_ap9_view`, skeleton, world model LOD0 to LOD4, and the magazine
  - every attachment (barrels, stocks, grips, silencers, mags) in view and world versions
  - textures, 1.3 GB of `.tif`
  - **114 view-model animations** (`xanim_export/t7_viewmodels/ap9/`): idle, fire, ADS fire,
    reload, empty reload, quick and extended reloads, pullout, putaway, first raise, sprint,
    crawl and the ADS up/down set for each optic
  - its asset definitions in `model_export/wpn_t7_smg_ap9.gdt`
- **First-person arms:** `c_t7_mp_warrior_viewhands` and `c_t7_mp_warrior2_viewhands`
  (`model_export/t7_characters/viewmodels/`).
- **Not complete:** the MR6 has textures and a GDT only, no model. The XR2 and the zombies
  Ray Gun have a skeleton only (the Ray Gun also has swim animations). **There's no Kuda
  model**, though `source_data/smg_standard.gdt` (the Kuda's internal name) may hold its
  weapon definition.
- **Documentation:** `docs_modtools/Weapon Fields.txt` describes the weapon stat fields.
  `share/raw/gamedata/weapons/` has the attachment tables.
- **Licence:** `modtools_license.txt` is in the depot. Read it before Phase 4. It may limit
  what its assets can be used for outside BO3.

### What this means for the plan

- **Phase 2 can be done entirely from the mod tools, using the AP9 instead of the MR6 or
  Kuda.** The plan's Phase 2 gate names the MR6 or Kuda, and neither is in the mod tools.
- **BO3 itself doesn't need installing for Phases 2 and 3.** Other BO3 weapons (KN-44,
  Kuda, MR6) would need extracting from the game's own files with the Windows-only tools.
  That's a later stretch goal, not a blocker.
- **Partial download:** `assets/filelists/ap9.txt` lists
  the AP9 kit, arms, GDTs, weapon field docs and licence: **455 files, 1.43 GB**. Fetch it
  with DepotDownloader's `-filelist` when Phase 2 starts.

## 3. Frame readback

The spike in `spikes/frame-readback/` uses wgpu 30.0.1 on Metal. It renders offscreen
world colour and depth, clears GUI and hand layers, copies everything GPU to CPU each frame
and writes it into a file laid out like `frames.shm`: three slots, rows bottom-up, `seq` odd
while writing. 600 frames per case after 60 warm-up frames. The Mac was on battery and
Elden Ring was not running.

| Size | Layers | Frames in flight | MB per frame | fps | Copy ms | Worst frame ms |
|---|---|---|---|---|---|---|
| 1920 x 1200 | 4 | 3 | 36.9 | 286.7 | 3.33 | 4.61 |
| 1920 x 1200 | 4 | 1 (synchronous) | 36.9 | 202.8 | 3.34 | 31.39 |
| 1280 x 800 | 4 | 3 | 16.4 | 530.3 | 1.76 | 2.82 |
| 1280 x 800 | 4 | 1 (synchronous) | 16.4 | 379.1 | 1.69 | 5.00 |
| 1920 x 1200 | 2 (GUI, hand) | 3 | 18.4 | 577.6 | 1.64 | 2.65 |
| 1280 x 800 | 2 (GUI, hand) | 3 | 8.2 | 1130.9 | 0.81 | 2.05 |

- At the largest size the readback costs about 3.3 ms of the 16.7 ms frame at 60 fps. With
  readback pipelined, the GPU wait drops to zero. Synchronous readback has spikes (one
  frame took 31 ms), so pipeline it. The Minecraft mod does the same with two alternating
  PBO sets.
- The written file was checked: header magic and version, slot headers, opaque colour, and
  depth in 0..0.984.
- **Still open:** the same test with Elden Ring running under CrossOver, sharing the GPU and
  memory bandwidth. That is Phase 1's job.

Findings for Phase 1, from the DLL source:

- **Colour layers are BGRA in memory**, not RGBA as the header comment says.
  `compositor.cpp:425` uploads them as `DXGI_FORMAT_B8G8R8A8_UNORM`, and the Java mod reads
  `GL_BGRA`. The spike uses `Bgra8Unorm`.
- **Depth must be OpenGL window depth from a finite near/far projection.** The DLL
  linearises it with the slot's `mcNear`/`mcFar` (`compositor.cpp:242`). Bevy renders
  reverse-Z with an infinite far plane, so the guest needs a small GPU pass that re-encodes
  depth, and can flip the rows at the same time.
- wgpu needs `bytes_per_row` to be a multiple of 256, so frame widths should be multiples of
  64 pixels or rows need padding.

Re-run (write to any file except `/tmp/ermc/frames.shm`):

```bash
cargo run --release --manifest-path spikes/frame-readback/Cargo.toml -- /tmp/frames-test.shm
```

## 4. Disk

**45.0 GB free** after installing Elden Ring (`diskutil`, container free space; it was
81.0 GB before). CrossOver takes 1.0 GB and the bottle 56 GB.

| Need | On disk | Source |
|---|---|---|
| Elden Ring base game | about 56 to 59 GB | Steam depots (main depot 55.04 GB) |
| Shadow of the Erdtree, only if owned | 16.13 GB | Steam depot 2778580 |
| CrossOver, bottle, Windows Steam | about 5 GB | plan estimate, not checked |
| Phase 0 tools and Minecraft | a few GB | setup guide |
| BO3 mod tools, full base depot | 28.15 GB | Steam depot 455131 |

The rest of Phase 0 (build tools, Java, Minecraft) is a few GB. The full mod tools would
leave about 12 GB, which is thin for swap on a 16 GB machine running two games. Options, from least to most effort:

1. **Partial download** of only the weapon files, once item 2's file list is known.
2. **Free space you can regenerate:** Xcode DerivedData 7.6 GB, old iOS DeviceSupport
   11 GB, simulator devices 14 GB.
3. **Personal files:** Movies 13 GB, Downloads 11 GB, STRAFTAT 6.8 GB, Cuphead 4.1 GB.
4. **An external drive** for the depot. None is attached now.

## Sources

- SteamCMD app info: [311210](https://api.steamcmd.net/v1/info/311210), [455130](https://api.steamcmd.net/v1/info/455130), [499270](https://api.steamcmd.net/v1/info/499270), [1245620](https://api.steamcmd.net/v1/info/1245620)
- [Modme wiki: Weapon Porting](https://wiki.modme.co/wiki/black_ops_3/guides/Weapon-Porting.html)
- [Steam discussion on the mod tools' additional assets](https://steamcommunity.com/app/455130/discussions/0/2217311444336952198)
- [Elden Ring 1.17.1 release report](https://mp1st.com/?p=314017)
