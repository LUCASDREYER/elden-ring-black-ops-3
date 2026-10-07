//! The view model's own motion, which BO3 does in its engine rather than in animations: a
//! walking bob in step with the feet, breathing at rest, the gun lagging behind the turns of
//! the mouse (BO3's sway settings from the weapon file), and a dip when landing.
//!
//! Works in eye space (inches; X forward, Y left, Z up) and returns one transform for the
//! whole rig.

use glam::{Mat4, Vec3};

/// The weapon file's sway settings (`swayLerpSpeed`, `swayMaxAngle`, `swayYawScale`,
/// `swayPitchScale`, `swayHorizScale`, `swayVertScale`, their `ads...` versions) and
/// `adsViewBobMult`.
#[derive(Clone, Debug)]
pub struct SwaySettings {
    pub lerp: f32,
    pub max_angle: f32,
    pub yaw_scale: f32,
    pub pitch_scale: f32,
    pub horiz: f32,
    pub vert: f32,
    pub ads_lerp: f32,
    pub ads_max_angle: f32,
    pub ads_yaw_scale: f32,
    pub ads_pitch_scale: f32,
    pub ads_horiz: f32,
    pub ads_vert: f32,
    pub ads_bob: f32,
}

impl Default for SwaySettings {
    /// `smg_standard_mp`'s values.
    fn default() -> Self {
        SwaySettings {
            lerp: 6.0,
            max_angle: 5.0,
            yaw_scale: -2.0,
            pitch_scale: -1.5,
            horiz: 0.3,
            vert: 0.25,
            ads_lerp: 5.5,
            ads_max_angle: 0.7,
            ads_yaw_scale: 0.15,
            ads_pitch_scale: 0.15,
            ads_horiz: 0.15,
            ads_vert: 0.15,
            ads_bob: 0.3,
        }
    }
}

/// What the player is doing this frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Moving {
    /// Ground speed, m/s.
    pub speed: f32,
    pub on_ground: bool,
    /// Vertical speed, m/s (negative falling).
    pub vel_y: f32,
    /// Camera yaw and pitch, radians.
    pub yaw: f32,
    pub pitch: f32,
    /// 0 hip .. 1 aiming.
    pub ads: f32,
    /// The sprint animation is playing (it carries its own motion).
    pub sprinting: bool,
}

/// Metres of ground per full bob cycle (two footfalls).
const STRIDE: f32 = 1.5;
const WALK_SPEED: f32 = 4.4;
/// Bob size at walking speed: side to side and down at each footfall (inches), roll (deg).
const BOB_SIDE: f32 = 0.42;
const BOB_DIP: f32 = 0.32;
const BOB_ROLL: f32 = 1.2;
const BOB_PITCH: f32 = 0.5;

#[derive(Default)]
pub struct Motion {
    phase: f32,
    amount: f32,
    sway_yaw: f32,
    sway_pitch: f32,
    land: f32,
    land_vel: f32,
    t: f32,
    prev: Option<(f32, f32)>,
    was_on_ground: bool,
    fall_speed: f32,
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

impl Motion {
    pub fn update(&mut self, dt: f32, m: &Moving, s: &SwaySettings) -> Mat4 {
        if dt <= 0.0 {
            return Mat4::IDENTITY;
        }
        self.t += dt;
        let ads = m.ads.clamp(0.0, 1.0);

        // Walking bob: in step with the distance covered. The sprint clip has its own.
        let target = if m.on_ground && !m.sprinting { (m.speed / WALK_SPEED).min(1.3) } else if m.sprinting { 0.25 } else { 0.0 };
        self.amount += (target - self.amount) * (8.0 * dt).min(1.0);
        self.phase = (self.phase + m.speed * dt * std::f32::consts::TAU / STRIDE) % (std::f32::consts::TAU * 64.0);
        let bob = self.amount * lerp(1.0, s.ads_bob, ads);
        let sp = self.phase.sin();
        let mut side = sp * BOB_SIDE * bob;
        let mut up = -sp.abs() * BOB_DIP * bob + 0.5 * BOB_DIP * bob;
        let roll = sp * BOB_ROLL * bob;
        let mut pitch_deg = (2.0 * self.phase).cos() * BOB_PITCH * bob * 0.5;

        // Breathing, smaller while aiming.
        let breathe = lerp(1.0, 0.3, ads);
        up += (self.t * 1.7).sin() * 0.05 * breathe;
        let mut yaw_deg = (self.t * 1.05).sin() * 0.12 * breathe;

        // Look sway: the gun trails the turn, as BO3's sway settings describe.
        let (dyaw, dpitch) = match self.prev {
            Some((py, pp)) => {
                let mut d = m.yaw - py;
                if d > std::f32::consts::PI {
                    d -= std::f32::consts::TAU;
                } else if d < -std::f32::consts::PI {
                    d += std::f32::consts::TAU;
                }
                (d.to_degrees(), (m.pitch - pp).to_degrees())
            }
            None => (0.0, 0.0),
        };
        self.prev = Some((m.yaw, m.pitch));
        // BO3's scales are per 60 Hz frame.
        let per_frame = 1.0 / (60.0 * dt);
        let max = lerp(s.max_angle, s.ads_max_angle, ads);
        let yaw_scale = lerp(s.yaw_scale.abs(), s.ads_yaw_scale.abs(), ads);
        let pitch_scale = lerp(s.pitch_scale.abs(), s.ads_pitch_scale.abs(), ads);
        let target_yaw = (dyaw * per_frame * yaw_scale).clamp(-max, max);
        let target_pitch = (dpitch * per_frame * pitch_scale).clamp(-max, max);
        let k = (lerp(s.lerp, s.ads_lerp, ads) * dt).min(1.0);
        self.sway_yaw += (target_yaw - self.sway_yaw) * k;
        self.sway_pitch += (target_pitch - self.sway_pitch) * k;
        // Turning right leaves the gun pointing left (+Z turn) and shifted left (+Y);
        // looking up leaves the muzzle low (+Y turn tips X toward -Z).
        yaw_deg += self.sway_yaw;
        pitch_deg += self.sway_pitch;
        side += self.sway_yaw * lerp(s.horiz, s.ads_horiz, ads) * 0.25;
        up -= self.sway_pitch * lerp(s.vert, s.ads_vert, ads) * 0.25;

        // Landing: a springy dip, deeper for longer falls.
        if !m.on_ground {
            self.fall_speed = self.fall_speed.max(-m.vel_y);
        } else if !self.was_on_ground && self.fall_speed > 2.0 {
            self.land_vel -= self.fall_speed.min(14.0) * 0.9;
            self.fall_speed = 0.0;
        }
        if m.on_ground {
            self.fall_speed = 0.0;
        }
        self.was_on_ground = m.on_ground;
        let (k_spring, damp) = (140.0, 2.0 * 140f32.sqrt() * 0.7);
        self.land_vel += (-k_spring * self.land - damp * self.land_vel) * dt;
        self.land += self.land_vel * dt;
        up += self.land * lerp(1.0, 0.4, ads);

        Mat4::from_translation(Vec3::new(0.0, side, up))
            * Mat4::from_rotation_z(yaw_deg.to_radians())
            * Mat4::from_rotation_y(pitch_deg.to_radians())
            * Mat4::from_rotation_x(roll.to_radians())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offset(m: Mat4) -> Vec3 {
        m.transform_point3(Vec3::ZERO)
    }

    #[test]
    fn walking_bobs_and_standing_barely_moves() {
        let s = SwaySettings::default();
        let mut still = Motion::default();
        let mut walk = Motion::default();
        let (mut still_max, mut walk_max) = (0.0f32, 0.0f32);
        for _ in 0..240 {
            let a = still.update(1.0 / 60.0, &Moving { on_ground: true, ..Default::default() }, &s);
            let b = walk.update(1.0 / 60.0, &Moving { on_ground: true, speed: 4.4, ..Default::default() }, &s);
            still_max = still_max.max(offset(a).length());
            walk_max = walk_max.max(offset(b).length());
        }
        assert!(still_max < 0.12, "breathing only: {still_max}");
        assert!(walk_max > 0.3, "walking bob: {walk_max}");
    }

    #[test]
    fn turning_right_swings_the_gun_left_and_aiming_damps_it() {
        let s = SwaySettings::default();
        let turn = |ads: f32| {
            let mut m = Motion::default();
            let mut yaw = 0.0;
            let mut last = Mat4::IDENTITY;
            for _ in 0..10 {
                yaw += 0.02;
                last = m.update(1.0 / 60.0, &Moving { on_ground: true, yaw, ads, ..Default::default() }, &s);
            }
            // The muzzle direction's sideways (Y, left) component.
            last.transform_vector3(Vec3::X).y
        };
        assert!(turn(0.0) > 0.02, "hip: gun trails to the left");
        assert!(turn(1.0).abs() < turn(0.0) * 0.2, "aiming: much steadier");
    }

    #[test]
    fn landing_dips() {
        let s = SwaySettings::default();
        let mut m = Motion::default();
        m.update(1.0 / 60.0, &Moving { on_ground: false, vel_y: -8.0, ..Default::default() }, &s);
        let mut lowest = 0.0f32;
        for _ in 0..30 {
            lowest = lowest.min(offset(m.update(1.0 / 60.0, &Moving { on_ground: true, ..Default::default() }, &s)).z);
        }
        assert!(lowest < -0.2, "dip {lowest}");
    }
}
