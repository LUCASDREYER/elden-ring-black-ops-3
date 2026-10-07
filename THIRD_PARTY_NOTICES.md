# Third-party notices

## PyCoD (CoDEmanX/blender-cod)

`crates/cod-assets` implements the XMODEL_BIN / XANIM_BIN container and block layout as documented by PyCoD's `xbin.py`, part of [CoDEmanX/blender-cod](https://github.com/CoDEmanX/blender-cod). No code is copied; the block tags and layouts follow it. Its licence:

```
MIT License

Copyright (c) 2017 

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## minecraft-crossover-bridge (justbustin)

Not vendored: the bridge is a separate clone. `crates/bridge-link` mirrors its shared-memory layout (`bridge_protocol.h`), and `patches/bridge-perf.patch` modifies its source. Its licence:

```
MIT License

Copyright (c) 2026 justbustin

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Rust crates

Dependencies (wgpu, winit, glam, cpal, lz4_flex, tiff, png, memmap2, bytemuck, pollster) keep their own licences, listed in `Cargo.lock` and on crates.io.

## Game content

None. Elden Ring and Call of Duty: Black Ops III, their assets and trademarks belong to their owners (Bandai Namco / FromSoftware; Activision / Treyarch). Nothing of theirs is in this repository; the program reads the player's own files.
