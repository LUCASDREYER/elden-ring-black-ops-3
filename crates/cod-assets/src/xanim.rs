//! `XANIM_BIN`: per-frame model-space transforms for named parts (bones), plus notetracks
//! ("fire", "reload_start"...). A bone's pose in a frame is absolute, like the bind pose in
//! an `XMODEL_BIN`, so skinning needs no parent chain for animated bones.

use std::path::Path;

use crate::xbin::{Blocks, decompress, tag};
use crate::{Result, Xform, format_err};

pub type PartPose = Xform;

#[derive(Clone, Debug)]
pub struct Note {
    pub frame: i32,
    pub name: String,
}

#[derive(Clone, Debug, Default)]
pub struct Anim {
    pub version: i16,
    /// Part (bone) names, in part index order.
    pub parts: Vec<String>,
    pub framerate: f32,
    /// `frames[f][part]`: `None` if the part has no key in that frame.
    pub frames: Vec<Vec<Option<PartPose>>>,
    pub notes: Vec<Note>,
}

impl Anim {
    pub fn load(path: impl AsRef<Path>) -> Result<Anim> {
        let path = path.as_ref();
        let file = std::fs::read(path)?;
        Anim::parse(&decompress(&file)?).map_err(|e| format_err(format!("{}: {e}", path.display())))
    }

    pub fn parse(data: &[u8]) -> Result<Anim> {
        let mut a = Anim { framerate: 30.0, ..Default::default() };
        let mut b = Blocks::new(data);
        // (frame, part) being filled; None inside the notetrack section.
        let mut active: Option<(usize, usize)> = None;
        let mut in_frames = false;

        while let Some(t) = b.peek() {
            match t {
                tag::COMMENT => {
                    b.comment()?;
                }
                tag::ANIM => {
                    b.int16()?;
                }
                tag::MODEL => return Err(format_err("this is a model, not an animation")),
                tag::VERSION => a.version = b.int16()?,
                tag::PART_COUNT => {
                    let n = b.int16()?.max(0) as usize;
                    a.parts = vec![String::new(); n];
                }
                tag::PART_INFO => {
                    let (i, name) = b.object_info()?;
                    if let Some(p) = a.parts.get_mut(i as usize) {
                        *p = name;
                    }
                }
                tag::FRAMERATE => a.framerate = b.int16()? as f32,
                tag::FRAME_COUNT => {
                    let n = b.int32()?.max(0) as usize;
                    a.frames.reserve(n);
                }
                tag::FRAME_INDEX => {
                    b.int32()?;
                    a.frames.push(vec![None; a.parts.len()]);
                    in_frames = true;
                    active = None;
                }
                tag::PART_INDEX => {
                    let p = b.int16()?.max(0) as usize;
                    active = match (in_frames, a.frames.len().checked_sub(1)) {
                        (true, Some(f)) if p < a.parts.len() => {
                            a.frames[f][p] = Some(Xform::default());
                            Some((f, p))
                        }
                        _ => None,
                    };
                }
                tag::OFFSET => {
                    let v = b.vec::<3>()?;
                    if let Some((f, p)) = active {
                        a.frames[f][p].as_mut().unwrap().offset = v;
                    }
                }
                tag::ROW_X | tag::ROW_Y | tag::ROW_Z => {
                    let v = b.short_vec3()?;
                    let r = match t {
                        tag::ROW_X => 0,
                        tag::ROW_Y => 1,
                        _ => 2,
                    };
                    if let Some((f, p)) = active {
                        a.frames[f][p].as_mut().unwrap().rows[r] = v;
                    }
                }
                tag::NOTETRACK_SECTION => {
                    b.int16()?;
                    in_frames = false;
                    active = None;
                }
                tag::NUM_TRACKS | tag::NUM_KEYS | tag::NOTETRACK => {
                    b.int16()?;
                }
                tag::NOTE_FRAME => {
                    let (frame, name) = b.note_frame()?;
                    a.notes.push(Note { frame, name });
                }
                tag::EXTRA => b.skip_extra()?,
                other => return Err(format_err(format!("unknown block {other:#06x} at {:#x}", b.pos()))),
            }
        }
        Ok(a)
    }

    pub fn duration(&self) -> f32 {
        (self.frames.len().saturating_sub(1)) as f32 / self.framerate.max(1.0)
    }

    pub fn part_index(&self, name: &str) -> Option<usize> {
        self.parts.iter().position(|p| p.eq_ignore_ascii_case(name))
    }

    /// The part's pose in `frame`, falling back to the nearest earlier key.
    pub fn pose(&self, frame: usize, part: usize) -> Option<PartPose> {
        let frame = frame.min(self.frames.len().checked_sub(1)?);
        (0..=frame).rev().find_map(|f| self.frames[f][part])
    }
}
