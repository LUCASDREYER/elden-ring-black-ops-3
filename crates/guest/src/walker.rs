//! Walking on Elden Ring's ground, Call of Duty style. The player is a capsule kept on the
//! surface with the bridge's raycasts: a ground ray and two rings of wall rays (knee and
//! chest height) cast from where the player stands. Answers arrive a game frame later, which
//! is fine at walking speed: the rings reach well past one frame's movement.

use bridge_link::RayHit;
use glam::Vec3;

/// Eye height above the feet, metres.
pub const EYE: f32 = 1.62;
const RADIUS: f32 = 0.35;
const WALK: f32 = 4.4;
const SPRINT: f32 = 6.8;
const ADS_SPEED: f32 = 0.55;
const GRAVITY: f32 = 18.0;
const JUMP: f32 = 5.4;
/// Highest ledge the player steps up without jumping.
const STEP: f32 = 0.5;
const RING: usize = 8;
const REACH: f32 = RADIUS + 0.55;
const KNEE: f32 = STEP + 0.1;
/// The ground ray reaches this far below the feet.
const GROUND_REACH: f32 = 8.0;
/// A fall this long, or this far below the last solid ground, is a fall out of the world
/// (through a lift the rays can't see, say): go back to that ground, like CoD's
/// out-of-bounds reset.
const LOST_FALL_TIME: f32 = 2.5;
const LOST_FALL_DEPTH: f32 = 25.0;
const CHEST: f32 = 1.25;

#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    /// -1..1, forward positive.
    pub forward: f32,
    /// -1..1, right positive.
    pub right: f32,
    pub sprint: bool,
    pub jump: bool,
    /// Aiming down the sights: slower, no sprint.
    pub aiming: bool,
}

#[derive(Clone, Copy, Debug)]
struct Wall {
    dir: Vec3,
    dist: f32,
    normal: Vec3,
}

pub struct Walker {
    pub feet: Vec3,
    pub vel_y: f32,
    pub on_ground: bool,
    /// From the last answered probe.
    ground: Option<f32>,
    walls: Vec<Wall>,
    /// Seconds without ground under the feet.
    airborne: f32,
    /// The last place the player stood on ground the rays confirmed.
    safe: Option<Vec3>,
    /// Set when the player was put back on safe ground; cleared by [`Walker::take_recovered`].
    recovered: bool,
}

fn ring_dir(k: usize) -> Vec3 {
    let a = (k % RING) as f32 * std::f32::consts::TAU / RING as f32;
    Vec3::new(a.sin(), 0.0, a.cos())
}

impl Walker {
    /// Rays per probe: ground, then the knee ring, then the chest ring.
    pub const PROBES: usize = 1 + 2 * RING;

    pub fn new(feet: Vec3) -> Walker {
        Walker { feet, vel_y: 0.0, on_ground: true, ground: Some(feet.y), walls: Vec::new(), airborne: 0.0, safe: None, recovered: false }
    }

    pub fn eye(&self) -> Vec3 {
        self.feet + Vec3::Y * EYE
    }

    /// The rays to cast from where the player stands now.
    pub fn probes(&self) -> Vec<([f32; 3], [f32; 3])> {
        let f = self.feet;
        let mut rays = vec![((f + Vec3::Y * 1.2).into(), (f - Vec3::Y * GROUND_REACH).into())];
        for h in [KNEE, CHEST] {
            for k in 0..RING {
                let from = f + Vec3::Y * h;
                rays.push((from.into(), (from + ring_dir(k) * REACH).into()));
            }
        }
        rays
    }

    /// Takes the answers to [`probes`](Self::probes) cast from `from`.
    pub fn absorb(&mut self, hits: &[Option<RayHit>], from: Vec3) {
        if hits.len() < Self::PROBES {
            return;
        }
        self.ground = hits[0].map(|h| h.pos[1]);
        // Confirmed footing: where to come back to after a fall out of the world.
        if self.on_ground && self.ground.is_some_and(|g| (g - from.y).abs() < 0.3) {
            self.safe = Some(Vec3::new(from.x, self.ground.unwrap(), from.z));
        }
        self.walls.clear();
        for (i, hit) in hits[1..Self::PROBES].iter().enumerate() {
            let Some(h) = hit else { continue };
            let dir = ring_dir(i);
            let p = Vec3::from(h.pos);
            let dist = Vec3::new(p.x - from.x, 0.0, p.z - from.z).length();
            let n = Vec3::from(h.normal);
            let normal = Vec3::new(n.x, 0.0, n.z).normalize_or(-dir);
            self.walls.push(Wall { dir, dist, normal });
        }
    }

    /// Moves the player for one frame; `yaw` is the camera's (0 looks along +Z).
    pub fn step(&mut self, yaw: f32, input: &Input, dt: f32) {
        let fwd = Vec3::new(yaw.sin(), 0.0, yaw.cos());
        let right = Vec3::new(yaw.cos(), 0.0, -yaw.sin());
        let wish = (fwd * input.forward + right * input.right).normalize_or_zero();
        let speed = if input.aiming {
            WALK * ADS_SPEED
        } else if input.sprint && input.forward > 0.0 {
            SPRINT
        } else {
            WALK
        };
        let mut mv = wish * speed * dt;

        // Slide along walls the rings found: drop the part of the move that goes into them.
        for w in &self.walls {
            if w.dist - RADIUS < mv.length() + 0.08 && mv.dot(w.dir) > 0.0 {
                let into = mv.dot(w.normal);
                if into < 0.0 {
                    mv -= w.normal * into;
                }
            }
        }
        self.feet += mv;

        if self.on_ground && input.jump {
            self.vel_y = JUMP;
            self.on_ground = false;
        }
        self.vel_y -= GRAVITY * dt;
        let y = self.feet.y + self.vel_y * dt;
        match self.ground {
            // Landing, or stepping up onto something low.
            Some(g) if g <= self.feet.y + STEP && y <= g => {
                self.feet.y = g;
                self.vel_y = 0.0;
                self.on_ground = true;
            }
            // Walking down a slope or a step: stay on it instead of floating off.
            Some(g) if self.on_ground && self.vel_y <= 0.0 && g < self.feet.y && self.feet.y - g < STEP => {
                self.feet.y = g;
                self.vel_y = 0.0;
            }
            _ => {
                self.feet.y = y;
                self.on_ground = false;
            }
        }
        self.airborne = if self.on_ground { 0.0 } else { self.airborne + dt };
        self.vel_y = self.vel_y.max(-40.0);
        if let Some(safe) = self.safe {
            if self.airborne > LOST_FALL_TIME || safe.y - self.feet.y > LOST_FALL_DEPTH {
                self.feet = safe;
                self.vel_y = 0.0;
                self.on_ground = true;
                self.ground = Some(safe.y);
                self.airborne = 0.0;
                self.recovered = true;
            }
        }
    }

    /// True once after the player was put back on safe ground.
    pub fn take_recovered(&mut self) -> bool {
        std::mem::take(&mut self.recovered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ground_hit(y: f32) -> Option<RayHit> {
        Some(RayHit { pos: [0.0, y, 0.0], normal: [0.0, 1.0, 0.0], attr: 0 })
    }

    #[test]
    fn walks_forward_on_flat_ground() {
        let mut w = Walker::new(Vec3::ZERO);
        let mut hits = vec![None; Walker::PROBES];
        hits[0] = ground_hit(0.0);
        w.absorb(&hits, w.feet);
        for _ in 0..60 {
            w.step(0.0, &Input { forward: 1.0, ..Default::default() }, 1.0 / 60.0);
        }
        assert!((w.feet.z - WALK).abs() < 0.05, "{:?}", w.feet);
        assert!(w.feet.y.abs() < 1e-4 && w.on_ground);
    }

    #[test]
    fn stops_at_a_wall_ahead() {
        let mut w = Walker::new(Vec3::ZERO);
        let mut hits = vec![None; Walker::PROBES];
        hits[0] = ground_hit(0.0);
        // Chest ring, direction 0 (+Z): a wall 0.4 m ahead facing back at us.
        hits[1 + RING] = Some(RayHit { pos: [0.0, CHEST, 0.4], normal: [0.0, 0.0, -1.0], attr: 0 });
        w.absorb(&hits, w.feet);
        w.step(0.0, &Input { forward: 1.0, ..Default::default() }, 1.0 / 60.0);
        assert!(w.feet.z.abs() < 1e-4, "walked into the wall: {:?}", w.feet);
        // Sideways along it is fine.
        w.step(0.0, &Input { right: 1.0, ..Default::default() }, 1.0 / 60.0);
        assert!(w.feet.x > 0.05);
    }

    #[test]
    fn falling_out_of_the_world_returns_to_safe_ground() {
        let mut w = Walker::new(Vec3::new(5.0, 10.0, 5.0));
        let mut hits = vec![None; Walker::PROBES];
        hits[0] = ground_hit(10.0);
        w.absorb(&hits, w.feet);
        // The floor vanishes (a lift the rays can't see): nothing below any more.
        w.absorb(&vec![None; Walker::PROBES], w.feet);
        let mut recovered = false;
        for _ in 0..300 {
            w.step(0.0, &Input::default(), 1.0 / 60.0);
            recovered |= w.take_recovered();
        }
        assert!(recovered, "never came back");
        assert!(w.feet.y > -20.0, "kept falling: {:?}", w.feet);
    }

    #[test]
    fn jumps_and_lands() {
        let mut w = Walker::new(Vec3::ZERO);
        let mut hits = vec![None; Walker::PROBES];
        hits[0] = ground_hit(0.0);
        w.absorb(&hits, w.feet);
        w.step(0.0, &Input { jump: true, ..Default::default() }, 1.0 / 60.0);
        let mut peak: f32 = 0.0;
        for _ in 0..120 {
            w.step(0.0, &Input::default(), 1.0 / 60.0);
            peak = peak.max(w.feet.y);
        }
        assert!(peak > 0.6 && peak < 1.2, "peak {peak}");
        assert!(w.on_ground && w.feet.y.abs() < 1e-4);
    }
}
