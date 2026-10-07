//! Mod-tools `.tif` textures, decoded to RGBA8.

use std::path::Path;

use tiff::decoder::{Decoder, DecodingResult, Limits};

use crate::{Result, format_err};

pub struct Texture {
    pub width: u32,
    pub height: u32,
    /// RGBA8, rows top-down.
    pub rgba: Vec<u8>,
}

impl Texture {
    pub fn load(path: impl AsRef<Path>) -> Result<Texture> {
        let path = path.as_ref();
        let err = |e: tiff::TiffError| format_err(format!("{}: {e}", path.display()));
        let file = std::io::BufReader::new(std::fs::File::open(path)?);
        let mut dec = Decoder::new(file).map_err(err)?.with_limits(Limits::unlimited());
        let (width, height) = dec.dimensions().map_err(err)?;
        let color = dec.colortype().map_err(err)?;
        let img = dec.read_image().map_err(err)?;
        let n = (width * height) as usize;

        // Normalise every sample to 8 bits.
        let samples: Vec<u8> = match img {
            DecodingResult::U8(v) => v,
            DecodingResult::U16(v) => v.into_iter().map(|x| (x >> 8) as u8).collect(),
            DecodingResult::F32(v) => v.into_iter().map(|x| (x.clamp(0.0, 1.0) * 255.0 + 0.5) as u8).collect(),
            _ => return Err(format_err(format!("{}: unsupported sample format", path.display()))),
        };
        let channels = samples.len() / n.max(1);
        let rgba = match (color, channels) {
            (_, 4) => samples,
            (_, 3) => samples.chunks_exact(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
            (_, 2) => samples.chunks_exact(2).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
            (_, 1) => samples.iter().flat_map(|&g| [g, g, g, 255]).collect(),
            _ => return Err(format_err(format!("{}: unsupported colour type {color:?}", path.display()))),
        };
        Ok(Texture { width, height, rgba })
    }

    /// Halves the size until both sides are at most `max` (box filter).
    pub fn shrink_to(mut self, max: u32) -> Texture {
        while self.width > max || self.height > max {
            let (w, h) = ((self.width / 2).max(1), (self.height / 2).max(1));
            let mut out = vec![0u8; (w * h * 4) as usize];
            for y in 0..h {
                for x in 0..w {
                    for c in 0..4 {
                        let mut sum = 0u32;
                        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                            let sx = (x * 2 + dx).min(self.width - 1);
                            let sy = (y * 2 + dy).min(self.height - 1);
                            sum += self.rgba[((sy * self.width + sx) * 4 + c) as usize] as u32;
                        }
                        out[((y * w + x) * 4 + c) as usize] = (sum / 4) as u8;
                    }
                }
            }
            self = Texture { width: w, height: h, rgba: out };
        }
        self
    }
}
