//! Readers for Black Ops 3 mod-tools source assets: `XMODEL_BIN`, `XANIM_BIN` and `.tif`
//! textures. No game files are bundled; everything is read from the player's own mod tools
//! download at runtime.
//!
//! The binary layout follows PyCoD's `xbin.py` (MIT licence, part of
//! `CoDEmanX/blender-cod`), which documents each block. This is an independent Rust
//! implementation of that format knowledge.
//!
//! Coordinates are left exactly as stored: Call of Duty units (inches), and the exporter's
//! axes. Converting to Elden Ring's space is the caller's job.

mod xbin;
pub mod gdt;
pub mod xanim;
pub mod xmodel;
pub mod texture;

pub use texture::Texture;
pub use xanim::{Anim, Note, PartPose};
pub use xmodel::{Bone, Corner, Face, Material, Model, Vertex};

use std::fmt;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Format(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Format(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

pub(crate) fn format_err(msg: impl Into<String>) -> Error {
    Error::Format(msg.into())
}

pub type Result<T> = std::result::Result<T, Error>;

/// A rotation (three axis rows, as stored) and a translation: a bone's or animation part's
/// transform in model space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xform {
    pub offset: [f32; 3],
    /// Rows X, Y and Z: the bone's local axes expressed in model space.
    pub rows: [[f32; 3]; 3],
}

impl Default for Xform {
    fn default() -> Self {
        Xform { offset: [0.0; 3], rows: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] }
    }
}

impl Xform {
    /// Column-major 4x4: the rows become the matrix's columns, the offset its translation.
    pub fn to_cols(&self) -> [[f32; 4]; 4] {
        let [x, y, z] = self.rows;
        let o = self.offset;
        [[x[0], x[1], x[2], 0.0], [y[0], y[1], y[2], 0.0], [z[0], z[1], z[2], 0.0], [o[0], o[1], o[2], 1.0]]
    }
}
