# Phase 1 evidence: Rust guest composites into Elden Ring

Recorded 2026-10-07, 04:46 to 04:49, on the M1 Pro (16 GB, macOS 26.6.2). Elden Ring
1.17.1 under CrossOver 26 (D3DMetal), windowed, Low settings, back buffer 1280 x 720. Bridge
`justbustin/minecraft-crossover-bridge` at `d1723873ec`. Guest: `crates/guest` in this repo.

## What was shown

- The guest attached to `bridge.shm`, waited for Elden Ring's camera, then drove it:
  state flags `0x4f` = camera valid, player valid, window valid, **camera overridden**,
  **compositing**.
- The DLL composited the guest's frames into Elden Ring's own:

```
[04:46:28.939] compositor: mapped frames.shm
[04:46:29.407] compositor: device objects ready
[04:46:29.421] compositor: frame textures 1280x720, upload 14400 KB x 3
[04:46:29.426] compositor: drawing Minecraft's frames into Elden Ring's (1280x720 into 1280x720)
[04:46:44.341] compositor: 3 full-screen depth candidate(s)
[04:46:44.342] compositor: occlusion uses depth candidate 0 (00007fa28b789c90)
```

- **Pose sync is exact:** each line covers 300 presented frames.

```
[04:48:03.248] sync: frames with the presented pose: exact 300, older 0, missing 0
...  (every sample from 04:48:03 to 04:49:04 is 300 / 0 / 0)
```

  The `missing 1` samples at 04:47:44 to 04:47:57 were while two guests ran at once (a
  restart left the old one alive); fixed, and `scripts/play.sh` now refuses a second guest.

- **Frame rate:** Elden Ring 51 fps and the guest 51 fps (`erctl.py fps`). The bridge's own
  perf lines show 49.6 to 51.1 fps with compositing, against 49.2 to 53.0 fps in the world
  before the guest started: no measurable cost.
- **Raycast:** the cube was dropped onto Elden Ring's ground with the ray block:
  `cube at Vec3(-36.33, 40.04, -180.85) (on the ground)`, Tarnished at y 39.08.

## Tests

`cargo test --release`: 6 passed. Includes `our_depth_linearises_like_the_compositor_expects`,
which runs our projection's depth through the compositor's `linMc` and gets the distance
back within 0.1% from 0.5 m to 900 m.

## The gate: the cube hides behind walls

**Passed, 2026-10-07 about 04:55, checked by eye** (the agent has no screen access). The
cube was visible in the open and disappeared behind walls when the camera flew around
them, with the default depth buffer (candidate 0); F3 was never pressed (guest log). Pose
sync stayed at 300 / 0 / 0 throughout.
