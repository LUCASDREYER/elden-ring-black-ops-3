//! The first-person weapon, built from the player's own BO3 mod tools files: the AP9 view
//! model, its magazine and the arms, skinned on the GPU and played with the AP9's
//! view-model animations (raise, idle, fire, reload).
//!
//! Animation space is the eye: Call of Duty units (inches), X forward, Y left, Z up.
//! [`to_view`] turns that into Elden Ring's left-handed camera space in metres.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bytemuck::{Pod, Zeroable};
use cod_assets::{Anim, Model, Texture, Xform};
use glam::{Mat3, Mat4, Quat, Vec3};

pub const INCH: f32 = 0.0254;

const GUN: &str = "model_export/weapons_t7/ap9_gremlin/wpn_t7_smg_ap9_view.XMODEL_BIN";
const MAG: &str = "model_export/weapons_t7/ap9_gremlin/wpn_t7_smg_ap9_mag_view.XMODEL_BIN";
const ARMS: &str = "model_export/t7_characters/viewmodels/c_t7_mp_warrior_viewhands.XMODEL_BIN";
const ANIM_DIR: &str = "xanim_export/t7_viewmodels/ap9";

use crate::stats::WeaponStats;
/// Zombies carry four magazines in reserve.
const RESERVE_MAGS: u32 = 4;
/// Cross-fade between animations.
const BLEND: f32 = 0.12;
/// At full ADS the eye, rear notch and front dot line up, but that sight line points 0.67 deg
/// below the camera's forward, where bullets go (the dot measured 10.2 px low at 720p in
/// `bin/snapshot`'s `a_ads.png`). Pitching the rig up about the eye puts the dot on centre.
const ADS_SIGHT_PITCH: f32 = 0.0118;
/// Field of view multiplier at full ADS (BO3's iron sights zoom a little).
pub const ADS_ZOOM: f32 = 0.82;
/// Textures bigger than this are halved on load (view-model detail doesn't need 4K).
const MAX_TEXTURE: u32 = 2048;

/// Where the mod tools files are: `BO3_MODTOOLS`, else the default download folder.
pub fn default_root() -> PathBuf {
    std::env::var_os("BO3_MODTOOLS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Developer/game-assets/bo3-modtools"))
}

/// Eye space (inches; X forward, Y left, Z up; right-handed) to Elden Ring camera space
/// (metres; X right, Y up, Z forward; left-handed). The mirror is what changes handedness.
pub fn to_view() -> Mat4 {
    Mat4::from_cols(
        glam::Vec4::new(0.0, 0.0, INCH, 0.0),  // eye X (forward) -> view Z
        glam::Vec4::new(-INCH, 0.0, 0.0, 0.0), // eye Y (left)    -> view -X
        glam::Vec4::new(0.0, INCH, 0.0, 0.0),  // eye Z (up)      -> view Y
        glam::Vec4::W,
    )
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct SkinVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub joints: [u32; 4],
    pub weights: [f32; 4],
}

pub struct MaterialInfo {
    pub name: String,
    /// Colour map, if the file was found.
    pub texture: Option<Texture>,
    /// Multiplies the texture (or is the colour when there's none).
    pub tint: [f32; 4],
    /// Alpha-tested decals.
    pub cutout: bool,
}

/// A run of indices drawn with one material.
pub struct Draw {
    pub first: u32,
    pub count: u32,
    pub material: usize,
}

struct BoneRig {
    name: String,
    parent: i32,
    bind_inv: Mat4,
    /// Bind pose relative to the parent, for bones the animation doesn't move.
    local: Mat4,
    bind: Mat4,
}

/// One skinned model.
pub struct Part {
    pub name: String,
    pub vertices: Vec<SkinVertex>,
    pub indices: Vec<u32>,
    pub draws: Vec<Draw>,
    pub materials: Vec<MaterialInfo>,
    bones: Vec<BoneRig>,
}

impl Part {
    pub fn bone_count(&self) -> usize {
        self.bones.len()
    }
}

fn xform_mat(x: &Xform) -> Mat4 {
    let rot = Mat3::from_cols(Vec3::from(x.rows[0]), Vec3::from(x.rows[1]), Vec3::from(x.rows[2]));
    Mat4::from_rotation_translation(Quat::from_mat3(&rot).normalize(), Vec3::from(x.offset))
}

fn xform_parts(x: &Xform) -> (Quat, Vec3) {
    let rot = Mat3::from_cols(Vec3::from(x.rows[0]), Vec3::from(x.rows[1]), Vec3::from(x.rows[2]));
    (Quat::from_mat3(&rot).normalize(), Vec3::from(x.offset))
}

/// Every `.tif` under the root, by lower-case file name.
fn texture_index(root: &Path) -> HashMap<String, PathBuf> {
    let mut out = HashMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("tif")) {
                if let Some(n) = p.file_name() {
                    out.insert(n.to_string_lossy().to_lowercase(), p);
                }
            }
        }
    }
    out
}

/// Colours for materials whose texture isn't available.
fn fallback_tint(name: &str) -> [f32; 4] {
    let n = name.to_lowercase();
    if n.contains("bullet") {
        [0.72, 0.55, 0.25, 1.0]
    } else if n.contains("tritium") {
        [0.4, 1.0, 0.45, 1.0]
    } else if n.contains("hand") || n.contains("glove") {
        [0.17, 0.17, 0.18, 1.0]
    } else if n.contains("arm") || n.contains("sleeve") {
        [0.24, 0.25, 0.21, 1.0]
    } else {
        [0.3, 0.3, 0.32, 1.0]
    }
}

/// BO3 multiplayer outfits are near-white textures the game tints per customisation. Tinted
/// here like a zombies crew member (Dempsey-style): olive-drab sleeves, brown leather gloves.
fn outfit_tint(material: &str) -> [f32; 4] {
    if !material.contains("warrior") {
        return [1.0; 4];
    }
    if material.contains("hand") {
        [0.36, 0.25, 0.17, 1.0]
    } else if material.contains("arm") || material.contains("clothes") || material.contains("straps") {
        [0.40, 0.41, 0.29, 1.0]
    } else {
        [0.55, 0.55, 0.52, 1.0]
    }
}

fn build_part(name: &str, model: &Model, textures: &HashMap<String, PathBuf>) -> Part {
    let bones: Vec<BoneRig> = model
        .bones
        .iter()
        .map(|b| {
            let bind = xform_mat(&b.bind);
            BoneRig { name: b.name.to_lowercase(), parent: b.parent, bind_inv: bind.inverse(), local: Mat4::IDENTITY, bind }
        })
        .collect();
    let mut bones = bones;
    for i in 0..bones.len() {
        let p = bones[i].parent;
        bones[i].local = if p >= 0 && (p as usize) < bones.len() { bones[p as usize].bind_inv * bones[i].bind } else { bones[i].bind };
    }

    let materials: Vec<MaterialInfo> = model
        .materials
        .iter()
        .map(|m| {
            let lower = m.name.to_lowercase();
            let file = m.images.get("color").map(|f| f.to_lowercase());
            // When the named file isn't in the depot, try the one named after the material
            // ("mtl_c_t7_warrior_body1_arm_lt_a1" -> "c_t7_warrior_body1_arm_lt_a_c.tif").
            let by_material = format!("{}_c.tif", lower.trim_start_matches("mtl_").trim_end_matches(|c: char| c.is_ascii_digit()));
            // Paint-shop overlays point at a fully transparent texture: nothing to draw.
            let texture = file
                .as_ref()
                .filter(|f| !f.starts_with("default_") && !lower.contains("paint"))
                .and_then(|f| textures.get(f).or_else(|| textures.get(&by_material)))
                .and_then(|p| match Texture::load(p) {
                    Ok(t) => Some(t.shrink_to(MAX_TEXTURE)),
                    Err(e) => {
                        eprintln!("viewmodel: {e}");
                        None
                    }
                });
            let tint = match &texture {
                Some(_) => outfit_tint(&lower),
                None => fallback_tint(&m.name),
            };
            MaterialInfo { name: m.name.clone(), texture, tint, cutout: lower.contains("decal") }
        })
        .collect();

    // One vertex per face corner: normals and UVs live on the corners.
    let mut by_material: Vec<Vec<u32>> = vec![Vec::new(); materials.len().max(1)];
    let mut vertices = Vec::with_capacity(model.faces.len() * 3);
    for face in &model.faces {
        let mat = (face.material as usize).min(by_material.len() - 1);
        if materials.get(mat).is_some_and(|m| m.name.to_lowercase().contains("paint")) {
            continue;
        }
        for c in &face.corners {
            let v = &model.verts[c.vert as usize];
            let mut w: Vec<(u16, f32)> = v.weights.clone();
            w.sort_by(|a, b| b.1.total_cmp(&a.1));
            w.truncate(4);
            let sum: f32 = w.iter().map(|x| x.1).sum();
            let mut joints = [0u32; 4];
            let mut weights = [0.0f32; 4];
            if sum <= 1e-6 {
                weights[0] = 1.0;
            }
            for (k, (bone, wt)) in w.iter().enumerate() {
                joints[k] = (*bone as u32).min(bones.len().saturating_sub(1) as u32);
                weights[k] = if sum > 1e-6 { wt / sum } else { 0.0 };
            }
            by_material[mat].push(vertices.len() as u32);
            vertices.push(SkinVertex { pos: v.pos, normal: c.normal, uv: c.uv, joints, weights });
        }
    }
    let mut indices = Vec::with_capacity(vertices.len());
    let mut draws = Vec::new();
    for (m, idx) in by_material.into_iter().enumerate() {
        if idx.is_empty() {
            continue;
        }
        draws.push(Draw { first: indices.len() as u32, count: idx.len() as u32, material: m });
        indices.extend(idx);
    }
    Part { name: name.to_string(), vertices, indices, draws, materials, bones }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Raise,
    Idle,
    Fire,
    FireAds,
    Reload,
    ReloadEmpty,
    SprintIn,
    Sprint,
    SprintOut,
}

impl Action {
    const ALL: [Action; 9] = [
        Action::Raise,
        Action::Idle,
        Action::Fire,
        Action::FireAds,
        Action::Reload,
        Action::ReloadEmpty,
        Action::SprintIn,
        Action::Sprint,
        Action::SprintOut,
    ];

    fn clip(self) -> &'static str {
        match self {
            Action::Raise => "vm_ap9_first_raise",
            Action::Idle => "vm_ap9_idle",
            Action::Fire => "vm_ap9_fire",
            Action::FireAds => "vm_ap9_fire_ads",
            Action::Reload => "vm_ap9_reload",
            Action::ReloadEmpty => "vm_ap9_reload_empty",
            Action::SprintIn => "vm_ap9_sprint_in",
            Action::Sprint => "vm_ap9_sprint_loop",
            Action::SprintOut => "vm_ap9_sprint_out",
        }
    }

    fn sprinting(self) -> bool {
        matches!(self, Action::SprintIn | Action::Sprint)
    }
}

/// What the player asks of the weapon this frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Controls {
    pub trigger: bool,
    pub reload: bool,
    pub aim: bool,
    pub sprint: bool,
}

/// What happened during an update, for sounds and gameplay.
#[derive(Default, Debug)]
pub struct Events {
    pub shots: u32,
    /// Notetracks passed this update ("clip_out", "clip_in"...).
    pub notes: Vec<String>,
}

pub struct Viewmodel {
    pub parts: Vec<Part>,
    clips: Vec<Anim>,
    clip_index: HashMap<&'static str, usize>,
    /// `[part][clip][bone]` -> animation part index.
    maps: Vec<Vec<Vec<Option<usize>>>>,
    pub action: Action,
    t: f32,
    /// The pose being faded out: (clip, time, fade time left).
    fade: Option<(usize, f32, f32)>,
    pub ammo: u32,
    /// Rounds carried beyond the magazine (zombies: four magazines).
    pub reserve: u32,
    cooldown: f32,
    /// 0 hip .. 1 sights at the eye.
    ads: f32,
    /// Eye-space shift of the whole rig at full ADS, from BO3's ADS clip.
    ads_offset: Vec3,
    pub stats: WeaponStats,
    /// The new magazine has been counted in this reload.
    refilled: bool,
    /// Bob, sway and the like, in eye space (see `motion.rs`).
    rig: Mat4,
}

impl Viewmodel {
    pub fn load(root: &Path, stats: WeaponStats) -> Result<Viewmodel, String> {
        let load_model = |rel: &str| Model::load(root.join(rel)).map_err(|e| e.to_string());
        let textures = texture_index(root);
        let parts = vec![
            build_part("ap9", &load_model(GUN)?, &textures),
            build_part("ap9 magazine", &load_model(MAG)?, &textures),
            build_part("arms", &load_model(ARMS)?, &textures),
        ];
        let mut clips = Vec::new();
        let mut clip_index = HashMap::new();
        for action in Action::ALL {
            let path = root.join(ANIM_DIR).join(format!("{}.XANIM_BIN", action.clip()));
            clips.push(Anim::load(&path).map_err(|e| e.to_string())?);
            clip_index.insert(action.clip(), clips.len() - 1);
        }
        let maps = parts
            .iter()
            .map(|p| {
                clips
                    .iter()
                    .map(|c| {
                        let names: HashMap<String, usize> = c.parts.iter().enumerate().map(|(i, n)| (n.to_lowercase(), i)).collect();
                        p.bones.iter().map(|b| names.get(&b.name).copied()).collect()
                    })
                    .collect()
            })
            .collect();
        // BO3's ADS clip moves only tag_torso, which carries the arms and the gun: its travel
        // from first to last frame is the ADS shift (sights centred, just below the eye).
        let ads_up = Anim::load(root.join(ANIM_DIR).join("vm_ap9_ads_base_up.XANIM_BIN")).map_err(|e| e.to_string())?;
        let ads_offset = ads_up
            .part_index("tag_torso")
            .and_then(|i| Some(Vec3::from(ads_up.pose(ads_up.frames.len().checked_sub(1)?, i)?.offset) - Vec3::from(ads_up.pose(0, i)?.offset)))
            .unwrap_or(Vec3::new(-3.6, 3.42, 0.75));
        Ok(Viewmodel {
            parts,
            clips,
            clip_index,
            maps,
            action: Action::Raise,
            t: 0.0,
            fade: None,
            ammo: stats.clip_size,
            reserve: stats.clip_size * RESERVE_MAGS,
            cooldown: 0.0,
            ads: 0.0,
            ads_offset,
            stats,
            refilled: false,
            rig: Mat4::IDENTITY,
        })
    }

    /// 0 at the hip, 1 aiming down the sights (eased).
    pub fn ads(&self) -> f32 {
        let a = self.ads.clamp(0.0, 1.0);
        a * a * (3.0 - 2.0 * a)
    }

    fn clip_of(&self, a: Action) -> usize {
        self.clip_index[a.clip()]
    }

    fn start(&mut self, a: Action) {
        let old = self.clip_of(self.action);
        self.fade = Some((old, self.t, BLEND));
        self.action = a;
        self.t = 0.0;
    }

    /// Advances the weapon one frame.
    pub fn update(&mut self, dt: f32, c: Controls) -> Events {
        let mut ev = Events::default();
        let clip = &self.clips[self.clip_of(self.action)];
        let fps = clip.framerate.max(1.0);
        let (f0, f1) = (self.t * fps, (self.t + dt) * fps);
        for n in &clip.notes {
            if (n.frame as f32) >= f0 && (n.frame as f32) < f1 {
                ev.notes.push(n.name.clone());
            }
        }
        let duration = clip.duration();
        self.t += dt;
        self.cooldown -= dt;
        if let Some((_, _, left)) = &mut self.fade {
            *left -= dt;
            if *left <= 0.0 {
                self.fade = None;
            }
        }
        let reloading = matches!(self.action, Action::Reload | Action::ReloadEmpty);
        // No aiming while sprinting or reloading, as in Call of Duty.
        let aim = c.aim && !self.action.sprinting() && !reloading && self.action != Action::Raise;
        self.ads = if aim {
            (self.ads + dt / self.stats.ads_in_time.max(0.01)).min(1.0)
        } else {
            (self.ads - dt / self.stats.ads_out_time.max(0.01)).max(0.0)
        };

        if matches!(self.action, Action::Raise) || reloading {
            // The new magazine counts partway through, as in BO3 (reloadTime).
            let count_at = if self.action == Action::ReloadEmpty { self.stats.reload_empty_time } else { self.stats.reload_time };
            if reloading && !self.refilled && self.t >= count_at.min(duration) {
                let take = (self.stats.clip_size - self.ammo).min(self.reserve);
                self.ammo += take;
                self.reserve -= take;
                self.refilled = true;
            }
            if self.t >= duration {
                self.start(Action::Idle);
            }
            return ev;
        }
        let can_reload = self.reserve > 0 && self.ammo < self.stats.clip_size;
        let want_reload = can_reload && (c.reload || (c.trigger && self.ammo == 0));
        if want_reload {
            self.refilled = false;
            self.start(if self.ammo == 0 { Action::ReloadEmpty } else { Action::Reload });
            return ev;
        }

        // Sprinting: in, loop, out. Firing or aiming drops out of it.
        let sprint = c.sprint && !c.trigger && !c.aim;
        match self.action {
            Action::SprintIn | Action::Sprint if !sprint => {
                self.start(Action::SprintOut);
                return ev;
            }
            Action::SprintIn if self.t >= duration => {
                self.start(Action::Sprint);
                return ev;
            }
            Action::Sprint => {
                if self.t >= duration {
                    self.t %= duration.max(1e-3);
                }
                return ev;
            }
            Action::SprintIn => return ev,
            Action::Idle | Action::SprintOut if sprint => {
                self.start(Action::SprintIn);
                return ev;
            }
            _ => {}
        }

        if c.trigger && self.ammo > 0 && self.cooldown <= 0.0 && self.action != Action::SprintOut {
            self.ammo -= 1;
            self.cooldown = self.stats.fire_time;
            ev.shots += 1;
            // Each shot restarts the fire animation, as in Call of Duty.
            self.fade = None;
            self.action = if self.ads > 0.5 { Action::FireAds } else { Action::Fire };
            self.t = 0.0;
        } else if matches!(self.action, Action::Fire | Action::FireAds | Action::SprintOut) && self.t >= duration {
            self.start(Action::Idle);
        } else if self.action == Action::Idle && self.t >= duration {
            self.t %= duration.max(1e-3);
        }
        ev
    }

    /// Per-bone pose of `part` in eye space for `clip` at time `t` (rotation, translation).
    fn sample(&self, part: usize, clip: usize, t: f32, out: &mut Vec<(Quat, Vec3)>) {
        let anim = &self.clips[clip];
        let p = &self.parts[part];
        let map = &self.maps[part][clip];
        let frames = anim.frames.len().max(1);
        let f = (t * anim.framerate).clamp(0.0, (frames - 1) as f32);
        let (f0, a) = (f.floor() as usize, f.fract());
        let f1 = (f0 + 1).min(frames - 1);
        out.clear();
        let mut world: Vec<Mat4> = Vec::with_capacity(p.bones.len());
        for (i, b) in p.bones.iter().enumerate() {
            let m = match map[i].and_then(|pi| Some((anim.pose(f0, pi)?, anim.pose(f1, pi)?))) {
                Some((x0, x1)) => {
                    let (q0, t0) = xform_parts(&x0);
                    let (q1, t1) = xform_parts(&x1);
                    Mat4::from_rotation_translation(q0.slerp(q1, a), t0.lerp(t1, a))
                }
                None if b.parent >= 0 && (b.parent as usize) < i => world[b.parent as usize] * b.local,
                None => b.bind,
            };
            world.push(m);
            let (_, r, t) = m.to_scale_rotation_translation();
            out.push((r, t));
        }
    }

    /// Skinning matrices for every part, in eye space (inches).
    pub fn skin(&self, out: &mut Vec<Vec<Mat4>>) {
        out.resize_with(self.parts.len(), Vec::new);
        let cur = self.clip_of(self.action);
        let mut a = Vec::new();
        let mut b = Vec::new();
        for (pi, part) in self.parts.iter().enumerate() {
            self.sample(pi, cur, self.t, &mut a);
            if let Some((clip, t, left)) = self.fade {
                self.sample(pi, clip, t, &mut b);
                let w = (left / BLEND).clamp(0.0, 1.0);
                for (x, y) in a.iter_mut().zip(&b) {
                    *x = (y.0.slerp(x.0, 1.0 - w), y.1.lerp(x.1, 1.0 - w));
                }
            }
            let aim = self.ads();
            // In eye space (X forward, Z up), a negative turn about Y lifts the muzzle end.
            let shift = self.rig * Mat4::from_rotation_y(-ADS_SIGHT_PITCH * aim) * Mat4::from_translation(self.ads_offset * aim);
            let dst = &mut out[pi];
            dst.clear();
            for (bone, (r, t)) in part.bones.iter().zip(&a) {
                dst.push(shift * Mat4::from_rotation_translation(*r, *t) * bone.bind_inv);
            }
        }
    }

    /// Jumps to an action at a time (for snapshots and tests).
    pub fn set(&mut self, a: Action, t: f32) {
        self.action = a;
        self.t = t;
        self.fade = None;
    }

    /// The rig's own motion this frame (bob, sway), applied on top of the animation.
    pub fn set_rig(&mut self, m: Mat4) {
        self.rig = m;
    }

    /// Max Ammo: a full magazine and a full reserve.
    pub fn refill_ammo(&mut self) {
        self.ammo = self.stats.clip_size;
        self.reserve = self.stats.clip_size * RESERVE_MAGS;
    }

    /// Jumps the sights to a position (for snapshots).
    pub fn set_ads(&mut self, ads: f32) {
        self.ads = ads;
    }

    /// The muzzle in eye space (inches), from the gun's `tag_flash`, for the flash.
    pub fn muzzle(&self, skins: &[Vec<Mat4>]) -> Option<Vec3> {
        let gun = self.parts.first()?;
        let i = gun.bones.iter().position(|b| b.name == "tag_flash")?;
        let bind = gun.bones[i].bind;
        Some((skins.first()?.get(i)? * bind).transform_point3(Vec3::ZERO))
    }

    pub fn clip_duration(&self, a: Action) -> f32 {
        self.clips[self.clip_of(a)].duration()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eye_space_maps_to_view_space() {
        let v = to_view();
        // 10 in forward, 2 in left, 3 in up.
        let p = v.transform_point3(Vec3::new(10.0, 2.0, 3.0));
        assert!((p - Vec3::new(-2.0 * INCH, 3.0 * INCH, 10.0 * INCH)).length() < 1e-6);
        assert!(v.determinant() < 0.0, "the mirror flips handedness");
    }
}
