# Phase 2 evidence: the AP9 in first person over Elden Ring

Recorded 2026-10-07, 05:00 to 05:15. Same machine and game setup as phase 1. Assets: the AP9
kit from the BO3 mod tools depot (455131, manifest 7421982922197022459), 453 files, 1.43 GB,
outside this folder.

## Reading the formats

`cargo run -p cod-assets --example inspect` on the real files:

- `wpn_t7_smg_ap9_view`: version 7, 14 bones, 10,517 verts, 19,552 faces, 17 materials
  (12 with colour maps found), rigid (1 weight per vertex).
- `wpn_t7_smg_ap9_mag_view`: 2 bones (`tag_clip`, `tag_bullet_animate`), 846 faces.
- `c_t7_mp_warrior_viewhands`: 110 bones, 28,712 faces, up to 9 weights per vertex (kept 4).
- `vm_ap9_idle` 3 frames, `vm_ap9_fire` 6, `vm_ap9_reload` 69 at 30 fps, 119 parts, notes
  `clip_out@18`, `clip_in@43`.
- Animation space is the eye: `tag_ads`, `tag_cambone` and `tag_camera` at the origin,
  `tag_weapon` at (7.44, -3.42, -7.05) inches.

## Rendering offline

`target/release/snapshot <dir>` renders through the same renderer and hand-layer readback
the guest sends to Elden Ring (PNGs kept outside the repo: they're renders of game assets).
Checked by eye: idle, fire, raise and three reload poses; a side camera showed the pistol-grip
magazine in place, the right hand on the grip, the left on the handguard, and the fresh
magazine in the left hand mid-reload.

## In game

- The gun showed in Elden Ring's frame, confirmed by eye ("i see the gun!").
- Fired and reloaded live: the guest log has 4 `clip_out`/`clip_in` pairs.
- Elden Ring 51 fps and guest 52 fps with the hand layer; pose sync 300/0/0 per sample (one
  startup sample 298/1/1).

## Gate

"The AP9 plays its idle, fire and reload in first person over Elden Ring": **passed.**
