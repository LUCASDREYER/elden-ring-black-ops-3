//! The AP9's numbers, from its BO3 weapon definition: `source_data/smg_standard.gdt`, entry
//! `smg_standard_mp`, the mod tools' sample SMG built on the AP9 (it names the AP9's
//! animations). Distances there are in inches; spreads in degrees.

use std::path::Path;

use crate::motion::SwaySettings;
use crate::viewmodel::INCH;

#[derive(Clone, Debug)]
pub struct WeaponStats {
    pub name: String,
    /// Seconds between shots.
    pub fire_time: f32,
    pub clip_size: u32,
    /// BO3 damage per tier (100-HP players) and where each tier ends, metres.
    pub damage: [f32; 5],
    pub damage_until: [f32; 5],
    pub head_multiplier: f32,
    /// Hip-fire cone, degrees: resting, most, per shot, recovery per second, while moving.
    pub hip_min: f32,
    pub hip_max: f32,
    pub hip_fire_add: f32,
    pub hip_decay: f32,
    pub hip_move_add: f32,
    pub ads_spread: f32,
    pub ads_in_time: f32,
    pub ads_out_time: f32,
    /// When the new magazine counts during a reload, seconds.
    pub reload_time: f32,
    pub reload_empty_time: f32,
    pub sway: SwaySettings,
}

impl Default for WeaponStats {
    /// The values in `smg_standard_mp` at the mod tools manifest read on 2026-10-07, used
    /// when the file isn't there.
    fn default() -> Self {
        WeaponStats {
            name: "AP9 (built-in)".into(),
            fire_time: 0.0833,
            clip_size: 30,
            damage: [35.0, 32.0, 29.0, 23.0, 23.0],
            damage_until: [100.0, 101.0, 1000.0, 1001.0, f32::MAX].map(|x| if x == f32::MAX { x } else { x * INCH }),
            head_multiplier: 1.1,
            hip_min: 2.0,
            hip_max: 5.0,
            hip_fire_add: 0.3,
            hip_decay: 4.0,
            hip_move_add: 5.0,
            ads_spread: 0.4,
            ads_in_time: 0.25,
            ads_out_time: 0.25,
            reload_time: 1.8,
            reload_empty_time: 2.3,
            sway: SwaySettings::default(),
        }
    }
}

impl WeaponStats {
    pub const FILE: &'static str = "source_data/smg_standard.gdt";
    pub const ENTRY: &'static str = "smg_standard_mp";

    /// Reads the weapon from the mod tools root; missing fields keep the defaults.
    pub fn load(root: &Path) -> Result<WeaponStats, String> {
        let gdt = cod_assets::gdt::load(root.join(Self::FILE)).map_err(|e| e.to_string())?;
        let e = gdt.get(Self::ENTRY).ok_or_else(|| format!("{} has no {}", Self::FILE, Self::ENTRY))?;
        let mut s = WeaponStats { name: Self::ENTRY.into(), ..Default::default() };
        let set = |field: &mut f32, key: &str| {
            if let Some(v) = e.f32(key) {
                *field = v;
            }
        };
        set(&mut s.fire_time, "fireTime");
        for (i, key) in ["damage", "damage2", "damage3", "damage4", "damage5"].iter().enumerate() {
            set(&mut s.damage[i], key);
        }
        // Tier i applies up to its range; the last tier everywhere beyond.
        for (i, key) in ["maxDamageRange", "damageRange2", "damageRange3", "damageRange4"].iter().enumerate() {
            if let Some(v) = e.f32(key) {
                s.damage_until[i] = v * INCH;
            }
        }
        set(&mut s.head_multiplier, "locHead");
        set(&mut s.hip_min, "hipSpreadStandMin");
        set(&mut s.hip_max, "hipSpreadMax");
        set(&mut s.hip_fire_add, "hipSpreadFireAdd");
        set(&mut s.hip_decay, "hipSpreadDecayRate");
        set(&mut s.hip_move_add, "hipSpreadMoveAdd");
        set(&mut s.ads_spread, "adsSpread");
        set(&mut s.ads_in_time, "adsTransInTime");
        set(&mut s.ads_out_time, "adsTransOutTime");
        set(&mut s.reload_time, "reloadTime");
        set(&mut s.reload_empty_time, "reloadEmptyTime");
        let w = &mut s.sway;
        for (field, key) in [
            (&mut w.lerp, "swayLerpSpeed"),
            (&mut w.max_angle, "swayMaxAngle"),
            (&mut w.yaw_scale, "swayYawScale"),
            (&mut w.pitch_scale, "swayPitchScale"),
            (&mut w.horiz, "swayHorizScale"),
            (&mut w.vert, "swayVertScale"),
            (&mut w.ads_lerp, "adsSwayLerpSpeed"),
            (&mut w.ads_max_angle, "adsSwayMaxAngle"),
            (&mut w.ads_yaw_scale, "adsSwayYawScale"),
            (&mut w.ads_pitch_scale, "adsSwayPitchScale"),
            (&mut w.ads_horiz, "adsSwayHorizScale"),
            (&mut w.ads_vert, "adsSwayVertScale"),
            (&mut w.ads_bob, "adsViewBobMult"),
        ] {
            set(field, key);
        }
        if let Some(c) = e.f32("clipSize") {
            s.clip_size = c.max(1.0) as u32;
        }
        Ok(s)
    }

    /// BO3 damage at `dist` metres.
    pub fn damage_at(&self, dist: f32) -> f32 {
        let i = self.damage_until.iter().position(|&r| dist <= r).unwrap_or(4);
        self.damage[i]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damage_tiers_by_distance() {
        let s = WeaponStats::default();
        assert_eq!(s.damage_at(1.0), 35.0); // within 100 in
        assert_eq!(s.damage_at(10.0), 29.0); // within 1000 in (25.4 m)
        assert_eq!(s.damage_at(40.0), 23.0);
    }
}
