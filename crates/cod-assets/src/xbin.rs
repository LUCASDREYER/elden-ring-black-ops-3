//! The container shared by `XMODEL_BIN` and `XANIM_BIN`: `*LZ4*`, the uncompressed size
//! (u32), then one LZ4 block. Inside, a stream of blocks, each starting with a 16-bit tag
//! and padded to a multiple of 4 bytes from its start.

use crate::{Result, format_err};

pub(crate) const MAGIC: &[u8; 5] = b"*LZ4*";

pub(crate) fn decompress(file: &[u8]) -> Result<Vec<u8>> {
    if file.len() < 9 || &file[..5] != MAGIC {
        return Err(format_err("not an x*_bin file (no *LZ4* header)"));
    }
    let size = u32::from_le_bytes(file[5..9].try_into().unwrap()) as usize;
    lz4_flex::block::decompress(&file[9..], size).map_err(|e| format_err(format!("LZ4: {e}")))
}

/// Block tags, from PyCoD's table.
pub(crate) mod tag {
    pub const COMMENT: u16 = 0xC355;
    pub const MODEL: u16 = 0x46C8;
    pub const ANIM: u16 = 0x7AAC;
    pub const VERSION: u16 = 0x24D1;

    pub const BONE_COUNT: u16 = 0x76BA;
    pub const COSMETIC_COUNT: u16 = 0x7836;
    pub const BONE_INFO: u16 = 0xF099;
    pub const BONE_INDEX: u16 = 0xDD9A;
    pub const OFFSET: u16 = 0x9383;
    pub const BONE_SCALE: u16 = 0x1C56;
    pub const ROW_X: u16 = 0xDCFD;
    pub const ROW_Y: u16 = 0xCCDC;
    pub const ROW_Z: u16 = 0xFCBF;

    pub const VERT_COUNT16: u16 = 0x950D;
    pub const VERT_COUNT32: u16 = 0x2AEC;
    pub const VERT_INDEX16: u16 = 0x8F03;
    pub const VERT_INDEX32: u16 = 0xB097;
    pub const WEIGHT_COUNT: u16 = 0xEA46;
    pub const WEIGHT: u16 = 0xF1AB;

    pub const FACE_COUNT: u16 = 0xBE92;
    pub const TRI: u16 = 0x562F;
    pub const TRI16: u16 = 0x6711;
    pub const NORMAL: u16 = 0x89EC;
    pub const COLOR: u16 = 0x6DD8;
    pub const UV: u16 = 0x1AD4;

    pub const OBJECT_COUNT: u16 = 0x62AF;
    pub const OBJECT_INFO: u16 = 0x87D4;

    pub const MATERIAL_COUNT: u16 = 0xA1B2;
    pub const MATERIAL_INFO: u16 = 0xA700;
    pub const MAT_VEC4: [u16; 5] = [0x6DAB, 0x37FF, 0x4265, 0x317C, 0xE593];
    pub const MAT_VEC2: [u16; 5] = [0xC835, 0xFE0C, 0x7E24, 0x7D76, 0x83C7];
    pub const MAT_PHONG: u16 = 0x5CD2;

    pub const PART_COUNT: u16 = 0x9279;
    pub const PART_INFO: u16 = 0x360B;
    pub const PART_INDEX: u16 = 0x745A;
    pub const FRAMERATE: u16 = 0x92D3;
    pub const FRAME_COUNT: u16 = 0xB917;
    pub const FRAME_INDEX: u16 = 0xC723;

    pub const NOTETRACK_SECTION: u16 = 0xC7F3;
    pub const NUM_TRACKS: u16 = 0x9016;
    pub const NUM_KEYS: u16 = 0x7A6C;
    pub const NOTETRACK: u16 = 0x4643;
    pub const NOTE_FRAME: u16 = 0x1675;
    pub const EXTRA: u16 = 0x6EEE;
}

/// Reads one block at a time. Each `read_*` consumes a whole block (tag included).
pub(crate) struct Blocks<'a> {
    data: &'a [u8],
    pos: usize,
}

fn padded(n: usize) -> usize {
    (n + 3) & !3
}

impl<'a> Blocks<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Blocks { data, pos: 0 }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    /// The next block's tag without consuming it.
    pub fn peek(&self) -> Option<u16> {
        self.data.get(self.pos..self.pos + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }

    fn bytes(&self, at: usize, n: usize) -> Result<&'a [u8]> {
        self.data
            .get(at..at + n)
            .ok_or_else(|| format_err(format!("block at {:#x} runs past the end", self.pos)))
    }

    fn u8_at(&self, at: usize) -> Result<u8> {
        Ok(self.bytes(at, 1)?[0])
    }
    fn i16_at(&self, at: usize) -> Result<i16> {
        Ok(i16::from_le_bytes(self.bytes(at, 2)?.try_into().unwrap()))
    }
    fn u16_at(&self, at: usize) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(at, 2)?.try_into().unwrap()))
    }
    fn i32_at(&self, at: usize) -> Result<i32> {
        Ok(i32::from_le_bytes(self.bytes(at, 4)?.try_into().unwrap()))
    }
    fn f32_at(&self, at: usize) -> Result<f32> {
        Ok(f32::from_le_bytes(self.bytes(at, 4)?.try_into().unwrap()))
    }
    fn floats<const N: usize>(&self, at: usize) -> Result<[f32; N]> {
        let mut out = [0.0; N];
        for (i, v) in out.iter_mut().enumerate() {
            *v = self.f32_at(at + i * 4)?;
        }
        Ok(out)
    }
    /// A NUL-terminated string at `at`; returns it and the offset just past the NUL.
    fn string_at(&self, at: usize) -> Result<(String, usize)> {
        let rest = self.data.get(at..).ok_or_else(|| format_err("string past the end"))?;
        let len = rest.iter().position(|&b| b == 0).ok_or_else(|| format_err("unterminated string"))?;
        Ok((String::from_utf8_lossy(&rest[..len]).into_owned(), at + len + 1))
    }

    fn finish(&mut self, len: usize) {
        self.pos += padded(len);
    }

    /// tag, i16
    pub fn int16(&mut self) -> Result<i16> {
        let v = self.i16_at(self.pos + 2)?;
        self.finish(4);
        Ok(v)
    }
    /// tag, u16
    pub fn uint16(&mut self) -> Result<u16> {
        let v = self.u16_at(self.pos + 2)?;
        self.finish(4);
        Ok(v)
    }
    /// tag, pad, i32
    pub fn int32(&mut self) -> Result<i32> {
        let v = self.i32_at(self.pos + 4)?;
        self.finish(8);
        Ok(v)
    }
    /// tag, pad, f32 x N
    pub fn vec<const N: usize>(&mut self) -> Result<[f32; N]> {
        let v = self.floats::<N>(self.pos + 4)?;
        self.finish(4 + 4 * N);
        Ok(v)
    }
    /// tag, i16 x 3 scaled to [-1, 1]
    pub fn short_vec3(&mut self) -> Result<[f32; 3]> {
        let mut v = [0.0; 3];
        for (i, x) in v.iter_mut().enumerate() {
            *x = self.i16_at(self.pos + 2 + i * 2)? as f32 / 32767.0;
        }
        self.finish(8);
        Ok(v)
    }
    /// tag, pad, text
    pub fn comment(&mut self) -> Result<String> {
        let (s, end) = self.string_at(self.pos + 4)?;
        self.finish(end - self.pos);
        Ok(s)
    }
    /// tag, pad, i32 index, i32 parent, name
    pub fn bone_info(&mut self) -> Result<(i32, i32, String)> {
        let index = self.i32_at(self.pos + 4)?;
        let parent = self.i32_at(self.pos + 8)?;
        let (name, end) = self.string_at(self.pos + 12)?;
        self.finish(end - self.pos);
        Ok((index, parent, name))
    }
    /// tag, i16 bone, f32 weight
    pub fn weight(&mut self) -> Result<(i16, f32)> {
        let bone = self.i16_at(self.pos + 2)?;
        let w = self.f32_at(self.pos + 4)?;
        self.finish(8);
        Ok((bone, w))
    }
    /// tag, u8 object, u8 material
    pub fn tri(&mut self) -> Result<(u16, u16)> {
        let o = self.u8_at(self.pos + 2)? as u16;
        let m = self.u8_at(self.pos + 3)? as u16;
        self.finish(4);
        Ok((o, m))
    }
    /// tag, pad, u16 object, u16 material
    pub fn tri16(&mut self) -> Result<(u16, u16)> {
        let o = self.u16_at(self.pos + 4)?;
        let m = self.u16_at(self.pos + 6)?;
        self.finish(8);
        Ok((o, m))
    }
    /// tag, pad, u8 x 4
    pub fn color(&mut self) -> Result<[f32; 4]> {
        let b = self.bytes(self.pos + 4, 4)?;
        let c = [b[0], b[1], b[2], b[3]].map(|v| v as f32 / 255.0);
        self.finish(8);
        Ok(c)
    }
    /// tag, i16 layer count, (f32, f32) per layer; returns the first layer
    pub fn uv(&mut self) -> Result<[f32; 2]> {
        let layers = self.i16_at(self.pos + 2)?.max(0) as usize;
        let uv = if layers > 0 { self.floats::<2>(self.pos + 4)? } else { [0.0; 2] };
        self.finish(4 + 8 * layers);
        Ok(uv)
    }
    /// tag, i16 index, name
    pub fn object_info(&mut self) -> Result<(i16, String)> {
        let index = self.i16_at(self.pos + 2)?;
        let (name, end) = self.string_at(self.pos + 4)?;
        self.finish(end - self.pos);
        Ok((index, name))
    }
    /// tag, i16 index, then name, type and image list, each NUL-terminated and 4-aligned
    pub fn material_info(&mut self) -> Result<(i16, String, String, String)> {
        let index = self.i16_at(self.pos + 2)?;
        let mut at = self.pos + 4;
        let mut next = || -> Result<String> {
            let (s, end) = self.string_at(at)?;
            at += padded(end - at);
            Ok(s)
        };
        let name = next()?;
        let kind = next()?;
        let images = next()?;
        self.finish(at - self.pos);
        Ok((index, name, kind, images))
    }
    /// tag, pad, i32 frame, text
    pub fn note_frame(&mut self) -> Result<(i32, String)> {
        let frame = self.i32_at(self.pos + 4)?;
        let (s, end) = self.string_at(self.pos + 8)?;
        self.finish(end - self.pos);
        Ok((frame, s))
    }
    /// tag, pad, 16 bytes of unknown data (xanim only)
    pub fn skip_extra(&mut self) -> Result<()> {
        self.bytes(self.pos, 20)?;
        self.finish(20);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decompress_roundtrip() {
        let raw: Vec<u8> = (0..1000u32).flat_map(|i| (i % 7).to_le_bytes()).collect();
        let mut file = MAGIC.to_vec();
        file.extend_from_slice(&(raw.len() as u32).to_le_bytes());
        file.extend_from_slice(&lz4_flex::block::compress(&raw));
        assert_eq!(decompress(&file).unwrap(), raw);
    }

    #[test]
    fn blocks_follow_padding() {
        // bone info: tag, pad, index 3, parent 1, "j_gun" (6 bytes with NUL -> 18 -> padded 20)
        let mut d = vec![0x99, 0xF0, 0, 0];
        d.extend_from_slice(&3i32.to_le_bytes());
        d.extend_from_slice(&1i32.to_le_bytes());
        d.extend_from_slice(b"j_gun\0\0\0");
        d.extend_from_slice(&[0xD1, 0x24, 7, 0]); // version 7
        let mut b = Blocks::new(&d);
        assert_eq!(b.bone_info().unwrap(), (3, 1, "j_gun".into()));
        assert_eq!(b.peek(), Some(tag::VERSION));
        assert_eq!(b.int16().unwrap(), 7);
        assert_eq!(b.peek(), None);
    }
}
