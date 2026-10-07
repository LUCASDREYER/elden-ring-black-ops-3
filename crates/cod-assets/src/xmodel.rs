//! `XMODEL_BIN`: a skinned mesh with its bind-pose skeleton.
//!
//! Bones are stored in model space (bind pose). Vertices carry a model-space position and
//! bone weights; triangles carry per-corner normal, colour and UV.

use std::collections::HashMap;
use std::path::Path;

use crate::xbin::{Blocks, decompress, tag};
use crate::{Result, Xform, format_err};

#[derive(Clone, Debug)]
pub struct Bone {
    pub name: String,
    /// -1 for a root.
    pub parent: i32,
    /// Bind pose in model space.
    pub bind: Xform,
    pub scale: [f32; 3],
}

#[derive(Clone, Debug, Default)]
pub struct Vertex {
    pub pos: [f32; 3],
    /// (bone index, weight)
    pub weights: Vec<(u16, f32)>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Corner {
    pub vert: u32,
    pub normal: [f32; 3],
    pub color: [f32; 4],
    pub uv: [f32; 2],
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Face {
    pub object: u16,
    pub material: u16,
    pub corners: [Corner; 3],
}

#[derive(Clone, Debug, Default)]
pub struct Material {
    pub name: String,
    pub kind: String,
    /// "color" -> file, "normal" -> file, ... (lower-case keys)
    pub images: HashMap<String, String>,
}

#[derive(Clone, Debug, Default)]
pub struct Model {
    pub version: i16,
    pub bones: Vec<Bone>,
    pub verts: Vec<Vertex>,
    pub faces: Vec<Face>,
    pub objects: Vec<String>,
    pub materials: Vec<Material>,
}

/// "color:a.tif normal:b.tif" -> map; a bare name means the colour map.
fn parse_images(s: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for item in s.split_whitespace() {
        if let Some((k, v)) = item.split_once(':') {
            out.insert(k.to_lowercase(), v.to_string());
        }
    }
    if out.is_empty() && !s.trim().is_empty() {
        out.insert("color".into(), s.trim().to_string());
    }
    out
}

enum Active {
    None,
    Bone(usize),
    Vert(usize),
    Corner(usize, usize),
    Material,
}

impl Model {
    pub fn load(path: impl AsRef<Path>) -> Result<Model> {
        let path = path.as_ref();
        let file = std::fs::read(path)?;
        Model::parse(&decompress(&file)?).map_err(|e| format_err(format!("{}: {e}", path.display())))
    }

    pub fn parse(data: &[u8]) -> Result<Model> {
        let mut m = Model::default();
        let mut b = Blocks::new(data);
        let mut active = Active::None;
        let mut in_faces = false;

        while let Some(t) = b.peek() {
            match t {
                tag::COMMENT => {
                    b.comment()?;
                }
                tag::MODEL => {
                    b.int16()?;
                }
                tag::ANIM => return Err(format_err("this is an animation, not a model")),
                tag::VERSION => m.version = b.int16()?,
                tag::BONE_COUNT => {
                    let n = b.int16()?.max(0) as usize;
                    m.bones = vec![Bone { name: String::new(), parent: -1, bind: Xform::default(), scale: [1.0; 3] }; n];
                }
                tag::COSMETIC_COUNT => {
                    b.int32()?;
                }
                tag::BONE_INFO => {
                    let (i, parent, name) = b.bone_info()?;
                    let bone = m.bones.get_mut(i as usize).ok_or_else(|| format_err(format!("bone index {i} out of range")))?;
                    bone.name = name;
                    bone.parent = parent;
                }
                tag::BONE_INDEX => {
                    let i = b.int16()? as usize;
                    if i >= m.bones.len() {
                        return Err(format_err(format!("bone index {i} out of range")));
                    }
                    active = Active::Bone(i);
                }
                tag::OFFSET => {
                    let v = b.vec::<3>()?;
                    match active {
                        Active::Bone(i) => m.bones[i].bind.offset = v,
                        Active::Vert(i) => m.verts[i].pos = v,
                        _ => return Err(format_err(format!("offset with nothing active at {:#x}", b.pos()))),
                    }
                }
                tag::BONE_SCALE => {
                    let v = b.vec::<3>()?;
                    if let Active::Bone(i) = active {
                        m.bones[i].scale = v;
                    }
                }
                tag::ROW_X | tag::ROW_Y | tag::ROW_Z => {
                    let v = b.short_vec3()?;
                    let r = match t {
                        tag::ROW_X => 0,
                        tag::ROW_Y => 1,
                        _ => 2,
                    };
                    if let Active::Bone(i) = active {
                        m.bones[i].bind.rows[r] = v;
                    }
                }
                tag::VERT_COUNT16 => {
                    let n = b.uint16()? as usize;
                    m.verts = vec![Vertex::default(); n];
                }
                tag::VERT_COUNT32 => {
                    let n = b.int32()?.max(0) as usize;
                    m.verts = vec![Vertex::default(); n];
                }
                tag::VERT_INDEX16 | tag::VERT_INDEX32 => {
                    let i = if t == tag::VERT_INDEX16 { b.uint16()? as usize } else { b.int32()?.max(0) as usize };
                    if i >= m.verts.len() {
                        return Err(format_err(format!("vertex index {i} out of range ({} verts)", m.verts.len())));
                    }
                    if in_faces {
                        let f = m.faces.len().checked_sub(1).ok_or_else(|| format_err("face corner before any face"))?;
                        let c = match active {
                            Active::Corner(face, c) if face == f => c + 1,
                            _ => 0,
                        };
                        if c > 2 {
                            return Err(format_err(format!("face {f} has more than 3 corners")));
                        }
                        m.faces[f].corners[c].vert = i as u32;
                        active = Active::Corner(f, c);
                    } else {
                        active = Active::Vert(i);
                    }
                }
                tag::WEIGHT_COUNT => {
                    b.int16()?;
                }
                tag::WEIGHT => {
                    let (bone, w) = b.weight()?;
                    if let Active::Vert(i) = active {
                        m.verts[i].weights.push((bone.max(0) as u16, w));
                    }
                }
                tag::FACE_COUNT => {
                    let n = b.int32()?.max(0) as usize;
                    m.faces.reserve(n);
                    in_faces = true;
                }
                tag::TRI | tag::TRI16 => {
                    let (object, material) = if t == tag::TRI { b.tri()? } else { b.tri16()? };
                    in_faces = true;
                    m.faces.push(Face { object, material, corners: Default::default() });
                    active = Active::None;
                }
                tag::NORMAL => {
                    let v = b.short_vec3()?;
                    if let Active::Corner(f, c) = active {
                        m.faces[f].corners[c].normal = v;
                    }
                }
                tag::COLOR => {
                    let v = b.color()?;
                    if let Active::Corner(f, c) = active {
                        m.faces[f].corners[c].color = v;
                    }
                }
                tag::UV => {
                    let v = b.uv()?;
                    if let Active::Corner(f, c) = active {
                        m.faces[f].corners[c].uv = v;
                    }
                }
                tag::OBJECT_COUNT => {
                    let n = b.int16()?.max(0) as usize;
                    m.objects = vec![String::new(); n];
                    active = Active::None;
                }
                tag::OBJECT_INFO => {
                    let (i, name) = b.object_info()?;
                    if let Some(o) = m.objects.get_mut(i as usize) {
                        *o = name;
                    }
                }
                tag::MATERIAL_COUNT => {
                    let n = b.int16()?.max(0) as usize;
                    m.materials = vec![Material::default(); n];
                }
                tag::MATERIAL_INFO => {
                    let (i, name, kind, images) = b.material_info()?;
                    let i = i as usize;
                    if let Some(mat) = m.materials.get_mut(i) {
                        *mat = Material { name, kind, images: parse_images(&images) };
                    }
                    active = Active::Material;
                }
                t if tag::MAT_VEC4.contains(&t) => {
                    b.vec::<4>()?;
                }
                t if tag::MAT_VEC2.contains(&t) => {
                    b.vec::<2>()?;
                }
                tag::MAT_PHONG => {
                    b.vec::<1>()?;
                }
                tag::EXTRA => b.skip_extra()?,
                other => return Err(format_err(format!("unknown block {other:#06x} at {:#x}", b.pos()))),
            }
        }
        Ok(m)
    }

    pub fn bone_index(&self, name: &str) -> Option<usize> {
        self.bones.iter().position(|b| b.name.eq_ignore_ascii_case(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_strings() {
        let m = parse_images("color:a_c.tif normal:a_n.tif");
        assert_eq!(m["color"], "a_c.tif");
        assert_eq!(m["normal"], "a_n.tif");
        assert_eq!(parse_images("plain.tif")["color"], "plain.tif");
    }
}
