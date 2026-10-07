//! Hitscan for the AP9. Each shot is a ray from the eye with a little spread. Elden Ring's
//! characters are tested straight away against the hitboxes the bridge publishes; the world
//! ray (walls, ground) comes back a frame later, and whichever is nearer takes the bullet.
//! Hits on characters go into the bridge's damage queue.

use bridge_link::{Bridge, Damage, Entity, RayHit, dmg};
use glam::Vec3;

use crate::stats::WeaponStats;

/// Bullet range, metres.
pub const RANGE: f32 = 150.0;
/// Bridge damage points per point of BO3 damage. The weapon's close-range 35 becomes 3
/// points; Elden Ring scales points by the square root of the target's max HP, so a basic
/// soldier takes about a dozen bullets and a 2,600-HP knight about 35.
const POINTS_PER_DAMAGE: f32 = 3.0 / 35.0;
/// World strikes (crates, pots) use the close-range damage.
const STRIKE_POINTS: f32 = 3.0;
/// The top part of a hitbox counts as the head.
const HEAD_FRACTION: f32 = 0.18;
/// World hits closer than this strike the world too (breaks crates and pots).
const STRIKE_REACH: f32 = 7.0;
const STRIKE_INTERVAL: f32 = 0.35;

/// A fired bullet waiting for its world ray.
#[derive(Clone, Debug)]
pub struct Shot {
    pub origin: Vec3,
    pub dir: Vec3,
    /// Nearest character on the line at fire time: (id, distance, point, headshot).
    target: Option<(u64, f32, Vec3, bool)>,
}

impl Shot {
    pub fn ray(&self) -> ([f32; 3], [f32; 3]) {
        (self.origin.into(), (self.origin + self.dir * RANGE).into())
    }
}

pub struct Combat {
    pub stats: WeaponStats,
    /// Seconds left on the hit marker; `crit` while the last hit was a headshot.
    pub hitmarker: f32,
    pub crit: bool,
    pub hits: u32,
    pub shots: u32,
    rng: u64,
    strike_cooldown: f32,
}

/// Slab test: distance along `dir` to where the ray enters the box, if it does.
fn ray_box(origin: Vec3, dir: Vec3, center: Vec3, half: Vec3) -> Option<f32> {
    let (mut t0, mut t1) = (0.0f32, f32::MAX);
    for k in 0..3 {
        let (o, d, lo, hi) = (origin[k], dir[k], center[k] - half[k], center[k] + half[k]);
        if d.abs() < 1e-8 {
            if o < lo || o > hi {
                return None;
            }
            continue;
        }
        let (a, b) = ((lo - o) / d, (hi - o) / d);
        t0 = t0.max(a.min(b));
        t1 = t1.min(a.max(b));
        if t0 > t1 {
            return None;
        }
    }
    Some(t0)
}

impl Combat {
    pub fn new(stats: WeaponStats) -> Combat {
        Combat { stats, hitmarker: 0.0, crit: false, hits: 0, shots: 0, rng: 0x9E37_79B9_7F4A_7C15, strike_cooldown: 0.0 }
    }

    fn rand(&mut self) -> f32 {
        // xorshift64*
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        ((self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32) / (1u64 << 24) as f32
    }

    /// Fires one bullet from `eye` along `forward` (unit) within a `spread_deg` cone,
    /// testing characters now.
    pub fn fire(&mut self, eye: Vec3, forward: Vec3, spread_deg: f32, entities: &[Entity]) -> Shot {
        self.shots += 1;
        // Uniform in a cone: random angle around the axis, square-root radius.
        let right = forward.cross(Vec3::Y).normalize_or(Vec3::X);
        let up = right.cross(forward);
        let r = spread_deg.to_radians() * self.rand().sqrt();
        let a = self.rand() * std::f32::consts::TAU;
        let dir = (forward + (right * a.cos() + up * a.sin()) * r.tan()).normalize();

        let mut target: Option<(u64, f32, Vec3, bool)> = None;
        for e in entities.iter().filter(|e| e.hostile() && !e.dead()) {
            let (c, h) = (Vec3::from(e.box_center), Vec3::from(e.box_half));
            if let Some(t) = ray_box(eye, dir, c, h) {
                if t <= RANGE && target.is_none_or(|x| t < x.1) {
                    let p = eye + dir * t;
                    let head = p.y > c.y + h.y * (1.0 - 2.0 * HEAD_FRACTION);
                    target = Some((e.id, t, p, head));
                }
            }
        }
        Shot { origin: eye, dir, target }
    }

    /// Settles a shot once its world ray is back: the nearer of character and wall wins.
    /// Returns the character hit, and whether it was a headshot.
    pub fn resolve(&mut self, bridge: &Bridge, shot: &Shot, world: Option<RayHit>) -> Option<(u64, bool)> {
        let wall = world.map(|h| (Vec3::from(h.pos) - shot.origin).length());
        match shot.target {
            Some((id, t, p, head)) if wall.is_none_or(|w| t < w) => {
                let mut amount = self.stats.damage_at(t) * POINTS_PER_DAMAGE;
                if head {
                    amount *= self.stats.head_multiplier;
                }
                bridge.push_damage(&Damage { id, amount, hit_pos: p.into(), flags: if head { dmg::CRITICAL } else { 0 } });
                self.hits += 1;
                self.hitmarker = 0.15;
                self.crit = head;
                Some((id, head))
            }
            _ => {
                if let (Some(w), Some(h)) = (wall, world) {
                    if w < STRIKE_REACH && self.strike_cooldown <= 0.0 {
                        bridge.push_damage(&Damage { id: 0, amount: STRIKE_POINTS, hit_pos: h.pos, flags: 0 });
                        self.strike_cooldown = STRIKE_INTERVAL;
                    }
                }
                None
            }
        }
    }

    pub fn update(&mut self, dt: f32) {
        self.hitmarker = (self.hitmarker - dt).max(0.0);
        self.strike_cooldown -= dt;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ray_box_hits_and_misses() {
        let c = Vec3::new(0.0, 1.0, 10.0);
        let h = Vec3::new(0.4, 0.9, 0.4);
        let t = ray_box(Vec3::new(0.0, 1.5, 0.0), Vec3::Z, c, h).unwrap();
        assert!((t - 9.6).abs() < 1e-4);
        assert!(ray_box(Vec3::new(2.0, 1.5, 0.0), Vec3::Z, c, h).is_none());
        assert!(ray_box(Vec3::new(0.0, 1.5, 0.0), -Vec3::Z, c, h).is_none());
    }

    #[test]
    fn shots_stay_in_the_cone_and_find_targets() {
        let mut c = Combat::new(WeaponStats::default());
        let e = Entity {
            id: 7,
            kind: bridge_link::ent::SMALL_MONSTER,
            box_center: [0.0, 1.0, 20.0],
            box_half: [0.5, 1.0, 0.5],
            ..Default::default()
        };
        let mut hits = 0;
        for _ in 0..200 {
            let s = c.fire(Vec3::new(0.0, 1.6, 0.0), Vec3::Z, 1.4, std::slice::from_ref(&e));
            assert!(s.dir.angle_between(Vec3::Z) <= 1.4f32.to_radians() + 1e-4);
            hits += s.target.is_some() as u32;
        }
        assert!(hits > 150, "a 1 m wide target at 20 m should take most hip-fire shots ({hits})");
    }
}
