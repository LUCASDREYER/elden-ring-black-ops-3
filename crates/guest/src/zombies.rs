//! Call of Duty Zombies rules on top of Elden Ring's fights: points for hits and kills,
//! rounds that advance with kills, and the odd Max Ammo.
//!
//! Classic scoring (Black Ops): +10 a hit that doesn't kill, +50 a kill, +100 a headshot
//! kill. Players start with 500. Solo rounds need 6, 8, 13, 18, 24 kills, then grow.

use std::collections::HashMap;

use bridge_link::Entity;

pub const START_POINTS: u32 = 500;
const HIT_POINTS: u32 = 10;
const KILL_POINTS: u32 = 50;
const HEADSHOT_KILL_POINTS: u32 = 100;
/// A kill counts if the character dies this long after our last hit on it.
const KILL_CREDIT_SECS: f32 = 3.0;
/// Chance a kill drops a Max Ammo.
const MAX_AMMO_CHANCE: f32 = 0.03;

/// A "+10" floating up from the score.
#[derive(Clone, Debug)]
pub struct Popup {
    pub points: u32,
    pub age: f32,
    /// Sideways drift, pixels at 720p.
    pub drift: f32,
}

#[derive(Debug, Default)]
pub struct ZombiesEvents {
    pub kills: u32,
    pub new_round: Option<u32>,
    pub max_ammo: bool,
}

pub struct Zombies {
    pub round: u32,
    pub kills_this_round: u32,
    pub points: u32,
    pub total_kills: u32,
    pub headshots: u32,
    pub popups: Vec<Popup>,
    /// Seconds since the round changed (the counter flashes).
    pub since_round_change: f32,
    /// Seconds since a Max Ammo, for the banner.
    pub since_max_ammo: f32,
    /// Characters we hit lately: id -> (seconds since, last hit was a headshot).
    recent: HashMap<u64, (f32, bool)>,
    rng: u64,
}

/// Kills needed to finish a solo round.
pub fn round_size(round: u32) -> u32 {
    match round {
        0 | 1 => 6,
        2 => 8,
        3 => 13,
        4 => 18,
        5 => 24,
        r => {
            let r = r as f32;
            (0.09 * r * r - 0.0029 * r + 23.958).floor() as u32
        }
    }
}

impl Default for Zombies {
    fn default() -> Self {
        Zombies {
            round: 1,
            kills_this_round: 0,
            points: START_POINTS,
            total_kills: 0,
            headshots: 0,
            popups: Vec::new(),
            since_round_change: 0.0,
            since_max_ammo: f32::MAX,
            recent: HashMap::new(),
            rng: 0x2545_F491_4F6C_DD1D,
        }
    }
}

impl Zombies {
    /// `key=value` lines: round, kills this round, points, total kills, headshots.
    pub fn save_string(&self) -> String {
        format!(
            "round={}\nkills_this_round={}\npoints={}\ntotal_kills={}\nheadshots={}\n",
            self.round, self.kills_this_round, self.points, self.total_kills, self.headshots
        )
    }

    /// Restores what [`save_string`](Self::save_string) wrote; unknown or bad lines are skipped.
    pub fn load_string(&mut self, text: &str) {
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let Ok(v) = v.trim().parse::<u32>() else { continue };
            match k.trim() {
                "round" => self.round = v.max(1),
                "kills_this_round" => self.kills_this_round = v,
                "points" => self.points = v,
                "total_kills" => self.total_kills = v,
                "headshots" => self.headshots = v,
                _ => {}
            }
        }
        self.since_round_change = 10.0;
    }

    fn earn(&mut self, points: u32) {
        self.points += points;
        let drift = (self.rand() - 0.5) * 30.0;
        self.popups.push(Popup { points, age: 0.0, drift });
    }

    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    /// A bullet of ours hit character `id`.
    pub fn hit(&mut self, id: u64, headshot: bool) {
        // Only hits on the living earn: the killing blow pays when the death shows up.
        self.recent.insert(id, (0.0, headshot));
        self.earn(HIT_POINTS);
    }

    /// Once a frame, with the characters Elden Ring published.
    pub fn update(&mut self, dt: f32, entities: &[Entity]) -> ZombiesEvents {
        let mut ev = ZombiesEvents::default();
        self.since_round_change += dt;
        self.since_max_ammo = (self.since_max_ammo + dt).min(1e6);
        for p in &mut self.popups {
            p.age += dt;
        }
        self.popups.retain(|p| p.age < 1.2);

        let mut dead = Vec::new();
        for (id, (since, _)) in self.recent.iter_mut() {
            *since += dt;
            let gone_dead = entities.iter().find(|e| e.id == *id).is_some_and(|e| e.dead() || e.hp <= 0.0);
            if gone_dead {
                dead.push(*id);
            }
        }
        self.recent.retain(|_, (since, _)| *since < KILL_CREDIT_SECS);
        for id in dead {
            let (_, head) = self.recent.remove(&id).unwrap_or((0.0, false));
            // The hit that killed already paid 10; the kill tops it up to 60 or 110.
            self.earn(if head { HEADSHOT_KILL_POINTS } else { KILL_POINTS });
            self.total_kills += 1;
            self.headshots += head as u32;
            self.kills_this_round += 1;
            ev.kills += 1;
            if self.rand() < MAX_AMMO_CHANCE {
                self.since_max_ammo = 0.0;
                ev.max_ammo = true;
            }
            if self.kills_this_round >= round_size(self.round) {
                self.round += 1;
                self.kills_this_round = 0;
                self.since_round_change = 0.0;
                ev.new_round = Some(self.round);
            }
        }
        ev
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bridge_link::ent;

    fn enemy(id: u64, hp: f32) -> Entity {
        Entity { id, kind: ent::SMALL_MONSTER, hp, max_hp: 100.0, flags: ent::WORLD_BOX, ..Default::default() }
    }

    #[test]
    fn hits_and_kills_pay_like_black_ops() {
        let mut z = Zombies::default();
        z.hit(1, false);
        z.update(0.016, &[enemy(1, 50.0)]);
        assert_eq!(z.points, 510);
        z.hit(1, true);
        let ev = z.update(0.016, &[enemy(1, 0.0)]);
        assert_eq!(ev.kills, 1);
        assert_eq!(z.points, 510 + 10 + 100, "headshot kill: 10 for the hit, 100 for the kill");
    }

    #[test]
    fn rounds_advance_with_kills() {
        let mut z = Zombies::default();
        for id in 0..6 {
            z.hit(id, false);
            let ev = z.update(0.016, &[enemy(id, 0.0)]);
            if id == 5 {
                assert_eq!(ev.new_round, Some(2));
            }
        }
        assert_eq!((z.round, z.kills_this_round), (2, 0));
        assert_eq!(round_size(2), 8);
        assert!(round_size(10) > round_size(6));
    }

    #[test]
    fn progress_survives_a_save_and_load() {
        let mut z = Zombies::default();
        z.round = 7;
        z.points = 4210;
        z.total_kills = 61;
        let mut back = Zombies::default();
        back.load_string(&z.save_string());
        assert_eq!((back.round, back.points, back.total_kills), (7, 4210, 61));
    }

    #[test]
    fn deaths_we_didnt_cause_dont_pay() {
        let mut z = Zombies::default();
        let ev = z.update(0.016, &[enemy(9, 0.0)]);
        assert_eq!((ev.kills, z.points), (0, START_POINTS));
    }
}
