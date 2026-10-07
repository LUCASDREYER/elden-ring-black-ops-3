//! Weapon and zombies sounds, synthesised at startup and mixed in software on a cpal output.
//!
//! These are not Black Ops 3's recordings: the mod tools ship no weapon audio (BO3's sounds
//! live in the game's own sound banks). Each sound is built from noise, filters and decaying
//! tones to sit where BO3's would: the AP9 names the Kuda's aliases
//! (`wpn_smg_standard_fire_plr`, `wpn_smg_plr_decay`, `wpn_generic_dryfire_plr`).

use std::f32::consts::TAU;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sfx {
    Fire,
    /// The room ringing out after a burst.
    Decay,
    DryFire,
    ClipOut,
    ClipIn,
    BoltPull,
    BoltRelease,
    AdsUp,
    AdsDown,
    Hit,
    RoundStart,
    MaxAmmo,
}

struct Voice {
    buf: Arc<Vec<f32>>,
    pos: usize,
    gain: f32,
}

struct Bank {
    fire: Vec<Arc<Vec<f32>>>,
    decay: Arc<Vec<f32>>,
    dry: Arc<Vec<f32>>,
    clip_out: Arc<Vec<f32>>,
    clip_in: Arc<Vec<f32>>,
    bolt_pull: Arc<Vec<f32>>,
    bolt_release: Arc<Vec<f32>>,
    ads_up: Arc<Vec<f32>>,
    ads_down: Arc<Vec<f32>>,
    hit: Arc<Vec<f32>>,
    round: Arc<Vec<f32>>,
    max_ammo: Arc<Vec<f32>>,
}

pub struct Audio {
    _stream: cpal::Stream,
    voices: Arc<Mutex<Vec<Voice>>>,
    bank: Bank,
    next_fire: usize,
}

/// Small helpers for building sounds sample by sample.
struct Synth {
    rate: f32,
    rng: u32,
}

impl Synth {
    fn noise(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        self.rng as f32 / u32::MAX as f32 * 2.0 - 1.0
    }

    /// One-pole low-pass coefficient for a cutoff in Hz.
    fn lp(&self, hz: f32) -> f32 {
        1.0 - (-TAU * hz / self.rate).exp()
    }

    fn len(&self, secs: f32) -> usize {
        (secs * self.rate) as usize
    }

    /// A decaying ring at `hz`, starting at `at` seconds.
    fn ring(&self, t: f32, at: f32, hz: f32, tau: f32) -> f32 {
        if t < at {
            0.0
        } else {
            (TAU * hz * (t - at)).sin() * (-(t - at) / tau).exp()
        }
    }
}

fn normalize(mut v: Vec<f32>, peak: f32) -> Arc<Vec<f32>> {
    let m = v.iter().fold(0.0f32, |a, x| a.max(x.abs())).max(1e-6);
    for x in &mut v {
        *x *= peak / m;
    }
    // A short fade-in so nothing starts with a pop.
    for (i, x) in v.iter_mut().take(24).enumerate() {
        *x *= i as f32 / 24.0;
    }
    Arc::new(v)
}

/// One shot: a supersonic crack, the 9 mm body, a low thump, the bolt cycling, a short room.
fn fire(s: &mut Synth, pitch: f32) -> Arc<Vec<f32>> {
    let n = s.len(0.45);
    let (mut lp_hi, mut lp_body, mut lp_tail) = (0.0f32, 0.0f32, 0.0f32);
    let (a_hi, a_body, a_tail) = (s.lp(5000.0), s.lp(1100.0 * pitch), s.lp(380.0));
    let mut phase = 0.0f32;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / s.rate;
        let (x1, x2) = (s.noise(), s.noise());
        lp_hi += a_hi * (x1 - lp_hi);
        let crack = (x1 - lp_hi) * (-t / 0.005).exp() * 1.1;
        lp_body += a_body * (x1 - lp_body);
        let body = lp_body * (-t / 0.05).exp() * 2.2;
        let f = (48.0 + 110.0 * (-t / 0.018).exp()) * pitch;
        phase += TAU * f / s.rate;
        let thump = phase.sin() * (-t / 0.075).exp() * 0.9;
        let bolt = (s.ring(t, 0.014, 3100.0 * pitch, 0.004) + 0.6 * s.ring(t, 0.016, 5400.0, 0.003)) * 0.25;
        lp_tail += a_tail * (x2 - lp_tail);
        let tail = lp_tail * (-t / 0.16).exp() * 1.4;
        out.push(crack + body + thump + bolt + tail);
    }
    normalize(out, 0.95)
}

fn decay(s: &mut Synth) -> Arc<Vec<f32>> {
    let n = s.len(1.1);
    let a = s.lp(260.0);
    let mut lp = 0.0f32;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / s.rate;
        lp += a * (s.noise() - lp);
        let env = (t / 0.03).min(1.0) * (-t / 0.38).exp();
        out.push(lp * env + (TAU * 44.0 * t).sin() * 0.25 * (-t / 0.3).exp());
    }
    normalize(out, 0.55)
}

/// A click: a couple of metallic rings and a little noise, with a body thud underneath.
fn click(s: &mut Synth, secs: f32, rings: &[(f32, f32, f32)], thud: f32, peak: f32) -> Arc<Vec<f32>> {
    let n = s.len(secs);
    let a = s.lp(220.0);
    let mut lp = 0.0f32;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / s.rate;
        let x = s.noise();
        lp += a * (x - lp);
        let mut v = lp * thud * (-t / 0.02).exp() * 3.0;
        for &(at, hz, tau) in rings {
            v += s.ring(t, at, hz, tau) + if t >= at { x * 0.3 * (-(t - at) / (tau * 0.6)).exp() } else { 0.0 };
        }
        out.push(v);
    }
    normalize(out, peak)
}

/// Band-limited noise with an attack and decay: slides, scrapes, cloth.
fn swish(s: &mut Synth, secs: f32, lo: f32, hi: f32, attack: f32, tau: f32, peak: f32) -> Arc<Vec<f32>> {
    let n = s.len(secs);
    let (a_lo, a_hi) = (s.lp(lo), s.lp(hi));
    let (mut l1, mut l2) = (0.0f32, 0.0f32);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / s.rate;
        let x = s.noise();
        l1 += a_lo * (x - l1);
        l2 += a_hi * (x - l2);
        let env = (t / attack).min(1.0) * (-(t - attack).max(0.0) / tau).exp();
        out.push((l2 - l1) * env);
    }
    normalize(out, peak)
}

/// A low, slow chord: the round changing.
fn bell(s: &mut Synth, notes: &[(f32, f32)], secs: f32, peak: f32) -> Arc<Vec<f32>> {
    let n = s.len(secs);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / s.rate;
        let mut v = 0.0;
        for &(hz, at) in notes {
            if t >= at {
                let u = t - at;
                let env = (u / 0.01).min(1.0) * (-u / (secs * 0.45)).exp();
                v += ((TAU * hz * u).sin() + 0.35 * (TAU * hz * 2.01 * u).sin() + 0.15 * (TAU * hz * 3.03 * u).sin()) * env;
            }
        }
        out.push(v * (1.0 + 0.15 * (TAU * 5.0 * t).sin()));
    }
    normalize(out, peak)
}

impl Bank {
    fn new(rate: f32) -> Bank {
        let mut s = Synth { rate, rng: 0x1234_5678 };
        Bank {
            fire: [1.0, 0.97, 1.03, 0.99, 1.02].iter().map(|&p| fire(&mut s, p)).collect(),
            decay: decay(&mut s),
            dry: click(&mut s, 0.08, &[(0.0, 2400.0, 0.003), (0.006, 4100.0, 0.002)], 0.2, 0.5),
            clip_out: {
                let slide = swish(&mut s, 0.12, 700.0, 3200.0, 0.01, 0.05, 0.5);
                let tick = click(&mut s, 0.12, &[(0.045, 1800.0, 0.006)], 0.4, 0.55);
                Arc::new(slide.iter().zip(tick.iter()).map(|(a, b)| a + b).collect())
            },
            clip_in: click(&mut s, 0.14, &[(0.004, 2600.0, 0.005), (0.032, 3300.0, 0.004)], 1.0, 0.75),
            bolt_pull: swish(&mut s, 0.12, 900.0, 4500.0, 0.07, 0.02, 0.5),
            bolt_release: click(&mut s, 0.14, &[(0.0, 2000.0, 0.008), (0.002, 4400.0, 0.005)], 1.0, 0.8),
            ads_up: swish(&mut s, 0.2, 200.0, 1500.0, 0.03, 0.07, 0.22),
            ads_down: swish(&mut s, 0.18, 180.0, 1200.0, 0.02, 0.06, 0.18),
            hit: click(&mut s, 0.05, &[(0.0, 3500.0, 0.0018)], 0.0, 0.28),
            round: bell(&mut s, &[(110.0, 0.0), (164.8, 0.08), (220.0, 0.16), (155.6, 0.9)], 2.6, 0.4),
            max_ammo: bell(&mut s, &[(392.0, 0.0), (523.3, 0.12), (659.3, 0.24), (784.0, 0.36)], 1.2, 0.35),
        }
    }
}

impl Audio {
    /// Opens the default output; `None` (and a message) if there's no usable device.
    pub fn new() -> Option<Audio> {
        let device = cpal::default_host().default_output_device()?;
        let supported = device.default_output_config().ok()?;
        let config = supported.config();
        let channels = config.channels as usize;
        let rate = config.sample_rate as f32;
        let voices: Arc<Mutex<Vec<Voice>>> = Arc::new(Mutex::new(Vec::new()));
        let mix_voices = voices.clone();
        let mix = move |out: &mut [f32]| {
            let mut vs = mix_voices.lock().unwrap();
            for frame in out.chunks_mut(channels) {
                let mut v = 0.0f32;
                for voice in vs.iter_mut() {
                    if let Some(x) = voice.buf.get(voice.pos) {
                        v += x * voice.gain;
                        voice.pos += 1;
                    }
                }
                // Soft clip: overlapping automatic fire can't crackle.
                let v = v.tanh();
                frame.iter_mut().for_each(|s| *s = v);
            }
            vs.retain(|voice| voice.pos < voice.buf.len());
        };
        let err = |e| eprintln!("audio: {e}");
        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => device.build_output_stream(config, move |out: &mut [f32], _: &cpal::OutputCallbackInfo| mix(out), err, None),
            cpal::SampleFormat::I16 => {
                let mut scratch = Vec::new();
                device.build_output_stream(
                    config,
                    move |out: &mut [i16], _: &cpal::OutputCallbackInfo| {
                        scratch.resize(out.len(), 0.0);
                        mix(&mut scratch);
                        for (o, s) in out.iter_mut().zip(&scratch) {
                            *o = (s * i16::MAX as f32) as i16;
                        }
                    },
                    err,
                    None,
                )
            }
            other => {
                eprintln!("audio: unsupported sample format {other:?}");
                return None;
            }
        }
        .ok()?;
        stream.play().ok()?;
        Some(Audio { _stream: stream, voices, bank: Bank::new(rate), next_fire: 0 })
    }

    pub fn play(&mut self, sfx: Sfx, gain: f32) {
        let b = &self.bank;
        let buf = match sfx {
            Sfx::Fire => {
                self.next_fire = (self.next_fire + 1) % b.fire.len();
                b.fire[self.next_fire].clone()
            }
            Sfx::Decay => b.decay.clone(),
            Sfx::DryFire => b.dry.clone(),
            Sfx::ClipOut => b.clip_out.clone(),
            Sfx::ClipIn => b.clip_in.clone(),
            Sfx::BoltPull => b.bolt_pull.clone(),
            Sfx::BoltRelease => b.bolt_release.clone(),
            Sfx::AdsUp => b.ads_up.clone(),
            Sfx::AdsDown => b.ads_down.clone(),
            Sfx::Hit => b.hit.clone(),
            Sfx::RoundStart => b.round.clone(),
            Sfx::MaxAmmo => b.max_ammo.clone(),
        };
        let mut vs = self.voices.lock().unwrap();
        if vs.len() < 48 {
            vs.push(Voice { buf, pos: 0, gain });
        }
    }

    /// The sound for an animation notetrack, if it has one.
    pub fn for_note(note: &str) -> Option<Sfx> {
        match note {
            "clip_out" => Some(Sfx::ClipOut),
            "clip_in" => Some(Sfx::ClipIn),
            "rechamber pull back" => Some(Sfx::BoltPull),
            "rechamber release" => Some(Sfx::BoltRelease),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sounds_are_finite_and_peak_where_asked() {
        let b = Bank::new(48_000.0);
        for buf in b.fire.iter().chain([&b.decay, &b.dry, &b.clip_in, &b.round]) {
            assert!(buf.iter().all(|x| x.is_finite()));
            let peak = buf.iter().fold(0.0f32, |a, x| a.max(x.abs()));
            assert!(peak > 0.1 && peak <= 0.96, "peak {peak}");
        }
        assert_eq!(Audio::for_note("clip_in"), Some(Sfx::ClipIn));
        assert_eq!(Audio::for_note("rmbnt#reload_large"), None);
    }
}
