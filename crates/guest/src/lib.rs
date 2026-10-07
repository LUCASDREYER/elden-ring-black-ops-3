//! The Black Ops 3 guest for the Elden Ring bridge: rendering, camera and the first-person
//! weapon. `main.rs` runs it against the live game; `bin/snapshot.rs` renders offline.

pub mod audio;
pub mod camera;
pub mod combat;
pub mod hud;
pub mod motion;
pub mod render;
pub mod stats;
pub mod viewmodel;
pub mod walker;
pub mod zombies;

use glam::Mat4;

/// Left-handed, depth 0 at `near` to 1 at `far`, NDC Y up: what wgpu expects.
pub fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    glam::camera::lh::proj::directx::perspective(fov_y, aspect, near, far)
}

/// Vertical field of view for the first-person weapon, independent of the world camera's.
pub const HAND_FOV_DEG: f32 = 55.0;

/// Camera space to clip space for the hand layer; `zoom` narrows it while aiming.
pub fn hand_proj(aspect: f32, zoom: f32) -> Mat4 {
    perspective((HAND_FOV_DEG * zoom).to_radians(), aspect, 0.01, 10.0)
}

/// Eye space (inches) to clip space for the hand layer.
pub fn hand_view_proj(aspect: f32) -> Mat4 {
    hand_proj(aspect, 1.0) * viewmodel::to_view()
}
