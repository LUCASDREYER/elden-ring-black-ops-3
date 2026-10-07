//! A free-flying camera in Elden Ring's world space (metres, left-handed: +X right, +Y up,
//! +Z forward). Yaw 0 looks along +Z; positive yaw turns right.

use std::collections::HashSet;

use glam::{Mat4, Vec3};
use winit::keyboard::KeyCode;

const SPEED: f32 = 4.0;
const FAST: f32 = 4.0;
const MOUSE: f32 = 0.0025;
const ARROWS: f32 = 1.6;
const PITCH_LIMIT: f32 = 1.5;

#[derive(Clone, Copy, Debug, Default)]
pub struct FlyCam {
    pub pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
}

impl FlyCam {
    pub fn looking(pos: Vec3, target: Vec3) -> Self {
        let d = (target - pos).normalize();
        FlyCam { pos, yaw: d.x.atan2(d.z), pitch: d.y.clamp(-1.0, 1.0).asin() }
    }

    pub fn forward(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        Vec3::new(sy * cp, sp, cy * cp)
    }

    /// Forward along the ground.
    pub fn flat_forward(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        Vec3::new(sy, 0.0, cy)
    }

    pub fn right(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        Vec3::new(cy, 0.0, -sy)
    }

    pub fn view(&self) -> Mat4 {
        glam::camera::lh::view::look_to_mat4(self.pos, self.forward(), Vec3::Y)
    }

    pub fn look(&mut self, dx: f32, dy: f32) {
        self.yaw += dx * MOUSE;
        self.pitch = (self.pitch - dy * MOUSE).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    /// Arrow keys turn the camera.
    pub fn turn(&mut self, keys: &HashSet<KeyCode>, dt: f32) {
        let held = |k| keys.contains(&k);
        let axis = |pos, neg| (held(pos) as i32 - held(neg) as i32) as f32;
        self.yaw += axis(KeyCode::ArrowRight, KeyCode::ArrowLeft) * ARROWS * dt;
        self.pitch = (self.pitch + axis(KeyCode::ArrowUp, KeyCode::ArrowDown) * ARROWS * dt).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    /// Recoil: the muzzle climbs and wanders a little.
    pub fn kick(&mut self, up: f32, side: f32) {
        self.pitch = (self.pitch + up).clamp(-PITCH_LIMIT, PITCH_LIMIT);
        self.yaw += side;
    }

    /// Free flight (noclip): turn, then move with W A S D, E / Q, Shift.
    pub fn update(&mut self, keys: &HashSet<KeyCode>, dt: f32) {
        self.turn(keys, dt);
        let held = |k| keys.contains(&k);
        let axis = |pos, neg| (held(pos) as i32 - held(neg) as i32) as f32;

        let mv = self.flat_forward() * axis(KeyCode::KeyW, KeyCode::KeyS)
            + self.right() * axis(KeyCode::KeyD, KeyCode::KeyA)
            + Vec3::Y * axis(KeyCode::KeyE, KeyCode::KeyQ);
        let speed = if held(KeyCode::ShiftLeft) || held(KeyCode::ShiftRight) { SPEED * FAST } else { SPEED };
        self.pos += mv.normalize_or_zero() * speed * dt;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looking_roundtrips_direction() {
        let pos = Vec3::new(1.0, 2.0, 3.0);
        let target = pos + Vec3::new(0.3, -0.2, 0.9);
        let cam = FlyCam::looking(pos, target);
        assert!((cam.forward() - (target - pos).normalize()).length() < 1e-5);
    }

    #[test]
    fn right_is_up_cross_forward() {
        // Left-handed: right = up x forward (as in the bridge's apply_camera).
        let cam = FlyCam { pos: Vec3::ZERO, yaw: 0.7, pitch: 0.0 };
        assert!((cam.right() - Vec3::Y.cross(cam.forward())).length() < 1e-5);
    }

    #[test]
    fn view_maps_forward_to_positive_z() {
        let cam = FlyCam { pos: Vec3::new(5.0, 1.0, -2.0), yaw: 1.1, pitch: 0.3 };
        let p = cam.view().transform_point3(cam.pos + cam.forward() * 10.0);
        assert!(p.x.abs() < 1e-4 && p.y.abs() < 1e-4 && (p.z - 10.0).abs() < 1e-4);
    }
}
