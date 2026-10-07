//! The guest: Black Ops 3's AP9 in the Lands Between, first person.
//!
//! Runs beside Elden Ring started with the bridge (`launch_er.sh`). Every time Elden Ring
//! presents a frame, the guest moves the player, renders from its eye, writes the layers into
//! `frames.shm` (world, hand, HUD), then hands Elden Ring that camera pose; Elden Ring renders
//! from it and composites the matching frame. The Tarnished stands in, hidden at the player's
//! feet: enemies attack it and its hits come back as damage to the player. Bullets are
//! hitscan against Elden Ring's characters and go into the bridge's damage queue.
//!
//! The guest's window is an overlay: borderless, transparent, always on top and glued over
//! Elden Ring's game area. It holds the keyboard and mouse (cursor locked and hidden) while
//! you play. F8 or Esc hands control to Elden Ring (its own camera, character, menus); F8 in
//! Elden Ring hands it back. Quit with Cmd+Q.
//!
//! Controls in BO3 mode:
//!   mouse         look            left click    fire (hold for automatic)       R   reload
//!   right click   aim down the sights (while the mouse is captured)
//!   W A S D       walk         Shift   sprint       Space   jump
//!   F             Elden Ring's action (open doors, pick up, rest at grace)
//!   V             walk / fly (noclip; the Tarnished reappears where it stood)
//!   E / Q         up / down while flying          arrows   look
//!   right-drag    look, while the mouse isn't captured
//!   C             put the cube on the ground 3 m ahead
//!   F5            hand the camera back to Elden Ring and start again from its view
//!   F1 / F2 / F3  depth test off, compositor test pattern, next occlusion depth buffer
//!   F8 / Esc      hand control to Elden Ring (F8 in Elden Ring comes back)
//!   Tab           release / recapture the mouse

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bridge_link::{Bridge, Control, Entity, FRAME_MAX_H, FRAME_MAX_W, FrameOut, Frames, ctrl, state};
use glam::{Mat4, Vec3};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId, WindowLevel};

use guest::audio::{Audio, Sfx};
use guest::camera::FlyCam;
use guest::motion::{Motion, Moving};
use guest::combat::{Combat, Shot};
use guest::hud::{Hud, HudState};
use guest::perspective;
use guest::stats::WeaponStats;
use guest::render::{HandFrame, MuzzleFlash, Renderer};
use guest::viewmodel::{self, Action, Controls, Viewmodel};
use guest::walker::{self, Walker};
use guest::zombies::Zombies;

const NEAR: f32 = 0.1;
const FAR: f32 = 1500.0;
/// Elden Ring's own field of view is used when it reports one in this range.
const DEFAULT_FOV: f32 = 48.0;
/// Player health lost per fraction of the Tarnished's max HP an Elden Ring hit takes.
const HURT_SCALE: f32 = 100.0;
/// Health comes back after this long without a hit, at this rate per second.
const REGEN_DELAY: f32 = 4.0;
const REGEN_RATE: f32 = 35.0;
/// Recoil per shot, radians.
const KICK_UP: f32 = 0.0035;
const KICK_SIDE: f32 = 0.0015;

enum Phase {
    /// No `bridge.shm` yet: Elden Ring isn't running through the bridge.
    NoBridge { retry_at: Instant },
    /// Waiting for Elden Ring's own camera (title screen, loading, death, or after F5).
    WaitCamera { not_before: u64 },
    Driving,
}

struct Link {
    bridge: Bridge,
    frames: Frames,
}

/// A ray batch in flight: the walker's probes first, then one ray per shot.
struct Pending {
    seq: u32,
    probes: usize,
    from: Vec3,
    shots: Vec<Shot>,
    sent: Instant,
}

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    link: Option<Link>,
    phase: Phase,
    /// The first-person weapon, if the mod tools files were found.
    vm: Option<Viewmodel>,
    skins: Vec<Vec<Mat4>>,
    cam: FlyCam,
    fov: f32,
    walk: bool,
    walker: Option<Walker>,
    combat: Combat,
    zombies: Zombies,
    audio: Option<Audio>,
    motion: Motion,
    /// For sounds: the trigger last frame, seconds since the last shot, aiming last frame.
    trigger_was: bool,
    since_shot: f32,
    decay_due: bool,
    aiming_was: bool,
    last_feet: Option<Vec3>,
    next_command_check: Instant,
    saved: String,
    next_save: Instant,
    hud: Hud,
    entities: Vec<Entity>,
    pending: Option<Pending>,
    queued: Vec<Shot>,
    health: f32,
    hurt_total: Option<f32>,
    since_hurt: f32,
    damage_flash: f32,
    /// Hip-fire spread added by shooting and by moving, degrees (BO3's spread model).
    fire_bloom: f32,
    move_bloom: f32,
    /// Current cone, degrees.
    spread: f32,
    life_seen: u32,
    prompt: String,
    cube: Vec3,
    /// Control handed to Elden Ring (F8): the overlay is hidden and nothing is driven.
    host_mode: bool,
    switch_seen: Option<u32>,
    overlay_on: bool,
    /// Elden Ring's client area the overlay was last glued to.
    glued: [i32; 4],
    busy_since: Option<Instant>,
    keys: HashSet<KeyCode>,
    trigger: bool,
    reload: bool,
    jump: bool,
    aim: bool,
    /// Seconds of muzzle flash left, and its spin.
    flash: f32,
    flash_spin: f32,
    looking: bool,
    captured: bool,
    depth_test: bool,
    depth_index: u32,
    test_pattern: bool,
    frame_id: u64,
    last_host: u64,
    last_tick: Instant,
    started: Instant,
    stats: Stats,
}

#[derive(Default)]
struct Stats {
    frames: u32,
    work: Duration,
    since: Option<Instant>,
    /// Per stage: simulate, HUD, render + readback, publish, preview.
    stage: [Duration; 5],
    worst: Duration,
}

/// Commands from outside, one per line: `max_ammo`, `points N`, `add_points N`, `round N`.
/// The guest reads the file, acts on it and deletes it.
const COMMAND_FILE: &str = "/tmp/ermc/guest.cmd";

/// Where the zombies progress is kept between runs.
fn save_path() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Library/Application Support/elden-ring-bo3/zombies.txt")
}

/// Switches for measuring what costs what (`GUEST_NO_PREVIEW=1` ...).
fn off(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| v != "0")
}

fn main() {
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        window: None,
        renderer: None,
        link: None,
        phase: Phase::NoBridge { retry_at: Instant::now() },
        vm: None,
        skins: Vec::new(),
        cam: FlyCam::default(),
        fov: DEFAULT_FOV,
        walk: true,
        walker: None,
        combat: Combat::new(WeaponStats::default()),
        zombies: Zombies::default(),
        audio: None,
        motion: Motion::default(),
        trigger_was: false,
        since_shot: f32::MAX,
        decay_due: false,
        aiming_was: false,
        last_feet: None,
        next_command_check: Instant::now(),
        saved: String::new(),
        next_save: Instant::now(),
        hud: Hud::new(1, 1),
        entities: Vec::new(),
        pending: None,
        queued: Vec::new(),
        health: 100.0,
        hurt_total: None,
        since_hurt: 0.0,
        damage_flash: 0.0,
        fire_bloom: 0.0,
        move_bloom: 0.0,
        spread: 0.0,
        life_seen: 0,
        prompt: String::new(),
        cube: Vec3::ZERO,
        host_mode: false,
        switch_seen: None,
        overlay_on: false,
        glued: [0; 4],
        busy_since: None,
        keys: HashSet::new(),
        trigger: false,
        reload: false,
        jump: false,
        aim: false,
        flash: 0.0,
        flash_spin: 0.0,
        looking: false,
        captured: false,
        depth_test: true,
        depth_index: 0,
        test_pattern: false,
        frame_id: 1,
        last_host: 0,
        last_tick: Instant::now(),
        started: Instant::now(),
        stats: Stats::default(),
    };
    println!("guest: waiting for Elden Ring (start it with launch_er.sh)");
    event_loop.run_app(&mut app).expect("run");
}

impl App {
    fn run_commands(&mut self) {
        let Ok(text) = std::fs::read_to_string(COMMAND_FILE) else { return };
        let _ = std::fs::remove_file(COMMAND_FILE);
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let mut words = line.split_whitespace();
            let (cmd, arg) = (words.next().unwrap_or(""), words.next().and_then(|a| a.parse::<u32>().ok()));
            match (cmd, arg) {
                ("max_ammo", _) => {
                    if let Some(vm) = &mut self.vm {
                        vm.refill_ammo();
                    }
                    self.zombies.since_max_ammo = 0.0;
                    if let Some(a) = &mut self.audio {
                        a.play(Sfx::MaxAmmo, 0.8);
                    }
                }
                ("points", Some(n)) => self.zombies.points = n,
                ("add_points", Some(n)) => self.zombies.points += n,
                ("round", Some(n)) => {
                    self.zombies.round = n.max(1);
                    self.zombies.kills_this_round = 0;
                    self.zombies.since_round_change = 0.0;
                }
                _ => {
                    println!("guest: unknown command {line:?}");
                    continue;
                }
            }
            println!("guest: command {line:?} done");
        }
    }

    fn save_progress(&mut self) {
        let text = self.zombies.save_string();
        if text != self.saved {
            let path = save_path();
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if std::fs::write(&path, &text).is_ok() {
                self.saved = text;
            }
        }
    }

    fn release(&self) {
        if let Some(link) = &self.link {
            link.bridge.write_control(&Control { flags: 0, pose_id: self.frame_id, ..Default::default() });
        }
    }

    /// Shows the overlay over Elden Ring and takes the keyboard and mouse.
    fn show_overlay(&mut self) {
        if let Some(w) = &self.window {
            w.set_visible(true);
            w.focus_window();
        }
        self.overlay_on = true;
        self.capture(true);
    }

    fn hide_overlay(&mut self) {
        self.capture(false);
        if let Some(w) = &self.window {
            w.set_visible(false);
        }
        self.overlay_on = false;
    }

    /// Keeps the overlay exactly over Elden Ring's client area. CrossOver (Retina mode off)
    /// maps one Windows pixel to one macOS point, with the same top-left origin.
    fn glue(&mut self, win: [i32; 4]) {
        if win[2] < 64 || win[3] < 64 || win == self.glued {
            return;
        }
        if let Some(w) = &self.window {
            w.set_outer_position(LogicalPosition::new(win[0] as f64, win[1] as f64));
            let _ = w.request_inner_size(LogicalSize::new(win[2] as f64, win[3] as f64));
            println!("guest: overlay over Elden Ring's {}x{} game area at {},{}", win[2], win[3], win[0], win[1]);
        }
        self.glued = win;
    }

    /// F8 / Esc: Elden Ring gets its camera, its character and the keyboard and mouse back.
    fn enter_host(&mut self) {
        if self.host_mode {
            return;
        }
        self.host_mode = true;
        self.release();
        self.hide_overlay();
        if let Some(l) = &self.link {
            l.bridge.request_host_focus();
        }
        if matches!(self.phase, Phase::Driving) {
            self.phase = Phase::WaitCamera { not_before: self.last_host + 5 };
        }
        println!("guest: control -> Elden Ring (F8 in Elden Ring comes back)");
    }

    /// F8 pressed in Elden Ring: take over again where the Tarnished stands.
    fn enter_bo3(&mut self) {
        if !self.host_mode {
            return;
        }
        self.host_mode = false;
        self.show_overlay();
        println!("guest: control -> BO3");
    }

    fn set_title(&self, text: &str) {
        if let Some(w) = &self.window {
            w.set_title(&format!("BO3 guest - {text}"));
        }
    }

    /// Called whenever the event loop is idle; does one frame per Elden Ring frame.
    fn tick(&mut self) {
        let now = Instant::now();
        if now >= self.next_command_check {
            self.next_command_check = now + Duration::from_millis(300);
            self.run_commands();
        }
        if now >= self.next_save {
            self.next_save = now + Duration::from_secs(5);
            self.save_progress();
        }
        if let Phase::NoBridge { retry_at } = self.phase {
            if now < retry_at {
                return;
            }
            match (Bridge::open(), Frames::open()) {
                (Ok(bridge), Ok(frames)) => {
                    bridge.announce();
                    self.last_host = bridge.host_heartbeat();
                    self.link = Some(Link { bridge, frames });
                    self.phase = Phase::WaitCamera { not_before: 0 };
                    println!("guest: attached to the Elden Ring bridge");
                    self.set_title("waiting for Elden Ring's camera (load into the world)");
                }
                (Err(e), _) | (_, Err(e)) => {
                    self.phase = Phase::NoBridge { retry_at: now + Duration::from_millis(500) };
                    self.set_title(&format!("waiting for Elden Ring ({e})"));
                }
            }
            return;
        }

        // F8 in Elden Ring (reported by the bridge): come back.
        let req = self.link.as_ref().unwrap().bridge.switch_requests();
        match self.switch_seen {
            Some(seen) if seen != req => {
                self.switch_seen = Some(req);
                self.enter_bo3();
            }
            None => self.switch_seen = Some(req),
            _ => {}
        }

        let link = self.link.as_ref().unwrap();
        let host = link.bridge.host_heartbeat();
        if host == self.last_host {
            // Elden Ring stopped presenting (closed, crashed, frozen): never keep the mouse
            // locked under an overlay with nothing behind it.
            if self.overlay_on && now - self.last_tick > Duration::from_secs(2) {
                self.hide_overlay();
                println!("guest: Elden Ring stopped drawing; overlay out of the way");
            }
            return;
        }
        self.last_host = host;
        let dt = (now - self.last_tick).as_secs_f32().min(0.1);
        self.last_tick = now;
        let Some(st) = link.bridge.state() else { return };
        let usable = st.has(state::CAMERA_VALID) && st.has(state::PLAYER_VALID) && !st.has(state::HOST_BUSY) && !st.has(state::PLAYER_DEAD);

        match self.phase {
            Phase::NoBridge { .. } => unreachable!(),
            Phase::WaitCamera { not_before } => {
                // Elden Ring's own screens for a while (the title screen): get out of the way.
                if !usable {
                    let since = *self.busy_since.get_or_insert(now);
                    if self.overlay_on && now - since > Duration::from_secs(3) {
                        self.hide_overlay();
                        if let Some(l) = &self.link {
                            l.bridge.request_host_focus();
                        }
                    }
                    return;
                }
                self.busy_since = None;
                if self.host_mode || host < not_before || st.has(state::CAM_OVERRIDDEN) {
                    return;
                }
                let pos = Vec3::from(st.cam_pos);
                let target = Vec3::from(st.cam_target);
                if (target - pos).length_squared() < 1e-6 {
                    return;
                }
                self.cam = FlyCam::looking(pos, target);
                self.cam.pitch = self.cam.pitch.clamp(-0.3, 0.3);
                self.fov = if (5.0..170.0).contains(&st.fov_y_deg) { st.fov_y_deg } else { DEFAULT_FOV };
                let player = Vec3::from(st.player_pos);
                self.walker = Some(Walker::new(player));
                self.life_seen = link.bridge.host_life();
                self.hurt_total = link.bridge.hunter_events().map(|e| e.total_damage);
                self.health = 100.0;
                self.pending = None;
                self.queued.clear();
                if self.cube == Vec3::ZERO {
                    self.place_cube(player + self.cam.flat_forward() * 3.0);
                }
                self.phase = Phase::Driving;
                self.glue(st.win);
                if !self.overlay_on {
                    self.show_overlay();
                }
                println!(
                    "guest: driving from {:.1?}, fov {:.1}, back buffer {}x{}, stage {}",
                    st.player_pos, self.fov, st.bb[0], st.bb[1], st.stage_id
                );
            }
            Phase::Driving => {
                if !usable {
                    // Loading screen or death: give the camera back and pick it up again after.
                    self.release();
                    self.phase = Phase::WaitCamera { not_before: host + 30 };
                    self.set_title(if st.has(state::PLAYER_DEAD) { "you died" } else { "Elden Ring is busy; waiting" });
                    return;
                }
                self.glue(st.win);
                if !self.overlay_on && !self.host_mode {
                    self.show_overlay(); // back after a stall
                }
                self.drive(&st, dt, now);
            }
        }
    }

    /// Answers to last frame's rays: the walker's probes and the shots' world rays.
    fn collect_rays(&mut self) {
        let link = self.link.as_ref().unwrap();
        let Some(p) = &self.pending else { return };
        if !link.bridge.rays_done(p.seq) {
            if p.sent.elapsed() > Duration::from_millis(500) {
                self.pending = None; // lost (Elden Ring reloaded); send again
            }
            return;
        }
        let p = self.pending.take().unwrap();
        let hits = link.bridge.read_hits(p.probes + p.shots.len());
        if let Some(w) = &mut self.walker {
            if p.probes > 0 {
                w.absorb(&hits[..p.probes], p.from);
            }
        }
        for (shot, hit) in p.shots.iter().zip(&hits[p.probes..]) {
            if let Some((id, head)) = self.combat.resolve(&link.bridge, shot, *hit) {
                self.zombies.hit(id, head);
                if let Some(a) = &mut self.audio {
                    a.play(Sfx::Hit, if head { 1.0 } else { 0.7 });
                }
            }
        }
    }

    fn send_rays(&mut self) {
        let link = self.link.as_ref().unwrap();
        if self.pending.is_some() || !link.bridge.rays_idle() {
            return;
        }
        let (probes, from) = match (&self.walker, self.walk) {
            (Some(w), true) => (w.probes(), w.feet),
            _ => (Vec::new(), Vec3::ZERO),
        };
        if probes.is_empty() && self.queued.is_empty() {
            return;
        }
        let mut rays = probes.clone();
        rays.extend(self.queued.iter().map(Shot::ray));
        if let Some(seq) = link.bridge.submit_rays(&rays) {
            let shots = std::mem::take(&mut self.queued);
            self.pending = Some(Pending { seq, probes: probes.len(), from, shots, sent: Instant::now() });
        }
    }

    fn health(&mut self, dt: f32) {
        let link = self.link.as_ref().unwrap();
        if let Some(ev) = link.bridge.hunter_events() {
            if let Some(prev) = self.hurt_total {
                if ev.total_damage > prev + 0.5 && ev.max_hp > 0.0 && self.walk {
                    let loss = (ev.total_damage - prev) / ev.max_hp * HURT_SCALE;
                    self.health -= loss;
                    self.since_hurt = 0.0;
                    self.damage_flash = 1.0;
                    println!("guest: hit for {:.0} Elden Ring HP -> {:.0} health", ev.total_damage - prev, self.health.max(0.0));
                }
            }
            self.hurt_total = Some(ev.total_damage);
        }
        self.since_hurt += dt;
        if self.since_hurt > REGEN_DELAY {
            self.health = (self.health + REGEN_RATE * dt).min(100.0);
        }
        self.damage_flash = (self.damage_flash - dt * 1.5).max(0.0);
        if self.health <= 0.0 && self.walk {
            // One life: the Tarnished dies too, and Elden Ring plays its death and respawn.
            println!("guest: you died");
            link.bridge.guest_died();
            self.health = 100.0;
        }
    }

    fn drive(&mut self, st: &bridge_link::GameState, dt: f32, now: Instant) {
        let t0 = Instant::now();
        let (w, h) = (st.bb[0], st.bb[1]);
        if w == 0 || h == 0 || w > FRAME_MAX_W || h > FRAME_MAX_H {
            self.release();
            self.set_title(&format!("Elden Ring's frame is {w}x{h}; it must be at most 1920x1200 (windowed)"));
            return;
        }
        self.collect_rays();

        // Elden Ring put the Tarnished somewhere new (a warp, a grace): follow it.
        let life = self.link.as_ref().unwrap().bridge.host_life();
        if life != self.life_seen {
            self.life_seen = life;
            self.walker = Some(Walker::new(Vec3::from(st.player_pos)));
            println!("guest: Elden Ring moved the Tarnished; following");
        }

        let ads = self.vm.as_ref().map(|v| v.ads()).unwrap_or(0.0);
        let mut sprinting = false;
        let mut moving = false;
        // Move.
        if self.walk {
            let jump = std::mem::take(&mut self.jump);
            let held = |k| self.keys.contains(&k);
            let axis = |p, n| (held(p) as i32 - held(n) as i32) as f32;
            let input = walker::Input {
                forward: axis(KeyCode::KeyW, KeyCode::KeyS),
                right: axis(KeyCode::KeyD, KeyCode::KeyA),
                sprint: held(KeyCode::ShiftLeft) || held(KeyCode::ShiftRight),
                jump,
                aiming: ads > 0.3,
            };
            sprinting = input.sprint && input.forward > 0.0 && !input.aiming;
            moving = input.forward != 0.0 || input.right != 0.0;
            self.cam.turn(&self.keys, dt);
            let walker = self.walker.get_or_insert_with(|| Walker::new(self.cam.pos - Vec3::Y * walker::EYE));
            walker.step(self.cam.yaw, &input, dt);
            if walker.take_recovered() {
                println!("guest: fell out of the world; back on solid ground at {:.1?}", walker.feet);
                self.pending = None; // its probes were cast from the fall
            }
            self.cam.pos = walker.eye();
        } else {
            self.cam.update(&self.keys, dt);
        }

        // Shoot.
        self.link.as_ref().unwrap().bridge.entities(&mut self.entities);
        let controls = Controls {
            trigger: self.trigger && self.captured_or_window(),
            reload: std::mem::take(&mut self.reload),
            aim: self.aim && self.captured,
            sprint: sprinting,
        };
        if let Some(vm) = &mut self.vm {
            let ev = vm.update(dt, controls);
            if let Some(a) = &mut self.audio {
                for _ in 0..ev.shots {
                    a.play(Sfx::Fire, 0.9);
                }
                for n in &ev.notes {
                    if let Some(sfx) = Audio::for_note(n) {
                        a.play(sfx, 0.8);
                    }
                }
                // Out of everything: the dry click, once per pull.
                if controls.trigger && !self.trigger_was && vm.ammo == 0 && vm.reserve == 0 {
                    a.play(Sfx::DryFire, 0.8);
                }
                let aiming = controls.aim && vm.ads() > 0.0;
                if aiming != self.aiming_was {
                    a.play(if aiming { Sfx::AdsUp } else { Sfx::AdsDown }, 0.6);
                }
                self.aiming_was = aiming;
            }
            self.trigger_was = controls.trigger;
            if ev.shots > 0 {
                self.since_shot = 0.0;
                self.decay_due = true;
            } else {
                self.since_shot += dt;
            }
            // The burst is over: let the room ring out.
            if self.decay_due && self.since_shot > vm.stats.fire_time * 1.5 {
                self.decay_due = false;
                if let Some(a) = &mut self.audio {
                    a.play(Sfx::Decay, 0.7);
                }
            }
            let aimed = vm.ads();
            let s = &vm.stats;
            // BO3: the hip cone grows with each shot and while moving, up to its max, and
            // recovers at the decay rate; aiming swaps it for the narrow ADS cone.
            self.move_bloom += ((moving as i32 as f32) - self.move_bloom) * (6.0 * dt).min(1.0);
            self.fire_bloom = (self.fire_bloom - s.hip_decay * dt).max(0.0);
            let hip = (s.hip_min + self.fire_bloom + self.move_bloom * s.hip_move_add).clamp(s.hip_min, s.hip_max);
            self.spread = hip + (s.ads_spread - hip) * aimed;
            for _ in 0..ev.shots {
                self.fire_bloom = (self.fire_bloom + vm.stats.hip_fire_add).min(vm.stats.hip_max);
                let shot = self.combat.fire(self.cam.pos, self.cam.forward(), self.spread, &self.entities);
                self.queued.push(shot);
                let kick = 1.0 - 0.4 * aimed;
                let side = if self.combat.shots % 2 == 0 { KICK_SIDE } else { -KICK_SIDE };
                self.cam.kick(KICK_UP * kick, side * kick);
                self.flash = 0.05;
                self.flash_spin = (self.flash_spin + 2.1) % std::f32::consts::TAU;
            }
            // The rig's own motion: bob in step with the ground covered, sway behind the mouse.
            let feet = self.walker.as_ref().map(|w| w.feet).unwrap_or(self.cam.pos);
            let speed = self.last_feet.map(|f| Vec3::new(feet.x - f.x, 0.0, feet.z - f.z).length() / dt.max(1e-3)).unwrap_or(0.0);
            self.last_feet = Some(feet);
            let (on_ground, vel_y) = match (&self.walker, self.walk) {
                (Some(w), true) => (w.on_ground, w.vel_y),
                _ => (false, 0.0),
            };
            let rig = self.motion.update(
                dt,
                &Moving { speed: speed.min(12.0), on_ground, vel_y, yaw: self.cam.yaw, pitch: self.cam.pitch, ads: aimed, sprinting },
                &vm.stats.sway,
            );
            vm.set_rig(rig);
            vm.skin(&mut self.skins);
        }
        self.flash = (self.flash - dt).max(0.0);
        self.combat.update(dt);
        let zm = self.zombies.update(dt, &self.entities);
        if zm.max_ammo {
            if let Some(vm) = &mut self.vm {
                vm.refill_ammo();
            }
            if let Some(a) = &mut self.audio {
                a.play(Sfx::MaxAmmo, 0.8);
            }
            println!("guest: MAX AMMO");
        }
        if let (Some(_), Some(a)) = (zm.new_round, &mut self.audio) {
            a.play(Sfx::RoundStart, 0.9);
        }
        if let Some(r) = zm.new_round {
            println!("guest: round {r} ({} points, {} kills)", self.zombies.points, self.zombies.total_kills);
        }
        self.send_rays();
        self.health(dt);
        self.prompt = self.link.as_ref().unwrap().bridge.prompt();

        let t_sim = t0.elapsed();
        // Draw.
        if self.hud.width != w || self.hud.height != h {
            self.hud = Hud::new(w, h);
        }
        let (ammo, mag, reserve, reloading, ads, hip_min) = match &self.vm {
            Some(vm) => (vm.ammo, vm.stats.clip_size, vm.reserve, matches!(vm.action, Action::Reload | Action::ReloadEmpty), vm.ads(), vm.stats.hip_min),
            None => (0, 0, 0, false, 0.0, 0.0),
        };
        // The crosshair opens with the cone: pixels per degree of the world view.
        let px_per_deg = h as f32 / self.fov;
        self.hud.draw(&HudState {
            weapon: "AP9",
            ammo,
            mag,
            reserve,
            reloading,
            aiming: ads > 0.5,
            bloom: (self.spread - hip_min).max(0.0) * px_per_deg,
            hitmarker: self.combat.hitmarker,
            crit: self.combat.crit,
            health: self.health,
            damage_flash: self.damage_flash,
            prompt: &self.prompt,
            round: self.zombies.round,
            since_round_change: self.zombies.since_round_change,
            points: self.zombies.points,
            popups: &self.zombies.popups,
            since_max_ammo: self.zombies.since_max_ammo,
        });

        // Aiming narrows both the world's and the weapon's field of view a little.
        let zoom = 1.0 + (viewmodel::ADS_ZOOM - 1.0) * ads;
        let fov = self.fov * zoom;
        let t_hud = t0.elapsed();
        let fov_rad = fov.to_radians();
        let view = self.cam.view();
        let proj = perspective(fov_rad, w as f32 / h as f32, NEAR, FAR);
        let spin = (now - self.started).as_secs_f32() * 0.6;
        let model = Mat4::from_translation(self.cube) * Mat4::from_rotation_y(spin);
        let hand_proj = guest::hand_proj(w as f32 / h as f32, zoom);
        let flash = match &self.vm {
            Some(vm) if self.flash > 0.0 => vm.muzzle(&self.skins).map(|m| MuzzleFlash {
                center: viewmodel::to_view().transform_point3(m),
                half_size: 0.07,
                intensity: (self.flash / 0.05).clamp(0.0, 1.0),
                rotation: self.flash_spin,
            }),
            _ => None,
        };
        let hand = self.vm.as_ref().filter(|_| !off("GUEST_NO_HAND")).map(|_| HandFrame {
            view_proj: hand_proj * viewmodel::to_view(),
            to_view: viewmodel::to_view(),
            proj: hand_proj,
            skins: &self.skins,
            flash,
        });

        let frame_id = self.frame_id;
        let hud = &self.hud;
        let no_hud = off("GUEST_NO_HUD");
        let renderer = self.renderer.as_mut().unwrap();
        let link = self.link.as_mut().unwrap();
        let mut t_publish = Duration::ZERO;
        renderer.render_frame(w, h, proj * view, model, hand.as_ref(), |layers| {
            let tp = Instant::now();
            link.frames.publish(&FrameOut {
                frame_id,
                width: w,
                height: h,
                near: NEAR,
                far: FAR,
                fov_y_deg: fov,
                world: layers.world,
                depth: layers.depth,
                hand: layers.hand,
                gui: if no_hud { None } else { Some(&hud.buf) },
                gui_stride: hud.stride(),
                row_stride: layers.stride,
            });
            t_publish = tp.elapsed();
        });
        let t_render = t0.elapsed() - t_hud - t_publish;
        // Only now may Elden Ring render this pose: the pixels for it are in place.
        let stand_in = if self.walk { ctrl::MOVE_HUNTER | ctrl::HIDE_HUNTER } else { 0 };
        let feet = self.walker.as_ref().map(|w| w.feet).unwrap_or(self.cam.pos);
        link.bridge.write_control(&Control {
            // NO_RELIGHT: the bridge would tint our pixels with a blur of the scene behind them,
            // which makes the gun look see-through; the hand shader lights it instead.
            flags: ctrl::OVERRIDE_CAMERA
                | ctrl::COMPOSITE
                | ctrl::NO_RELIGHT
                | stand_in
                | if self.depth_test { 0 } else { ctrl::NO_DEPTH_TEST },
            pose_id: frame_id,
            cam_pos: self.cam.pos.into(),
            cam_target: (self.cam.pos + self.cam.forward()).into(),
            cam_up: [0.0, 1.0, 0.0],
            fov_y_deg: fov,
            hunter_pos: feet.into(),
            // The bridge takes Minecraft's yaw: forward = (-sin y, 0, -cos y).
            hunter_yaw_deg: (self.cam.yaw + std::f32::consts::PI).to_degrees(),
            pose_lag: 1,
            depth_index: self.depth_index,
            ..Default::default()
        });
        link.bridge.beat();
        self.frame_id += 1;

        let t_before_preview = t0.elapsed();
        let aspect = renderer.preview_aspect();
        let preview_proj = perspective(fov_rad, aspect, NEAR, FAR);
        let preview_hand_proj = guest::hand_proj(aspect, zoom);
        let preview_hand = hand.map(|h| HandFrame { view_proj: preview_hand_proj * viewmodel::to_view(), proj: preview_hand_proj, ..h });
        if !off("GUEST_NO_PREVIEW") {
            renderer.render_preview(preview_proj * view, model, preview_hand.as_ref());
        }
        let t_all = t0.elapsed();

        self.stats.frames += 1;
        self.stats.work += t_all;
        self.stats.worst = self.stats.worst.max(t_all);
        for (acc, d) in self.stats.stage.iter_mut().zip([t_sim, t_hud - t_sim, t_render, t_publish, t_all - t_before_preview]) {
            *acc += d;
        }
        let since = *self.stats.since.get_or_insert(now);
        if now - since >= Duration::from_secs(1) {
            let fps = self.stats.frames as f32 / (now - since).as_secs_f32();
            let ms = self.stats.work.as_secs_f32() * 1000.0 / self.stats.frames as f32;
            let compositing = if st.has(state::COMPOSITING) { "" } else { ", NOT compositing yet" };
            let mode = if self.walk { "walking" } else { "flying" };
            let mouse = if self.captured { "" } else { ", Tab to aim" };
            let hits = format!("{}/{} hits", self.combat.hits, self.combat.shots);
            self.set_title(&format!("{mode}, {:.0} hp, {hits}, {fps:.0} fps, {ms:.1} ms{compositing}{mouse}", self.health));
            let n = self.stats.frames as f32;
            let st: Vec<String> = self.stats.stage.iter().map(|d| format!("{:.1}", d.as_secs_f32() * 1000.0 / n)).collect();
            println!(
                "guest: perf {fps:.0} fps, {ms:.1} ms/frame (worst {:.1}): sim {} hud {} render+readback {} publish {} preview {}",
                self.stats.worst.as_secs_f32() * 1000.0,
                st[0], st[1], st[2], st[3], st[4]
            );
            self.stats = Stats::default();
        }
    }

    /// Fire only while the mouse is captured or the guest window is in front, so clicking
    /// into the window doesn't loose a burst.
    fn captured_or_window(&self) -> bool {
        self.captured || self.window.as_ref().is_some_and(|w| w.has_focus())
    }

    /// Puts the cube on Elden Ring's ground below `at` (found with the bridge's raycast).
    fn place_cube(&mut self, at: Vec3) {
        // A blocking cast shares the ray slot with the frame's batch: drop that batch first.
        self.pending = None;
        let ground = self.link.as_ref().and_then(|l| {
            let ray = ((at + Vec3::Y * 4.0).into(), (at - Vec3::Y * 40.0).into());
            l.bridge.raycast(&[ray], Duration::from_millis(300))?.into_iter().next().flatten()
        });
        self.cube = match ground {
            Some(hit) => Vec3::from(hit.pos) + Vec3::Y * 0.5,
            None => at,
        };
        println!(
            "guest: cube at {:.2?} ({})",
            self.cube,
            if ground.is_some() { "on the ground" } else { "no ground found below; floating" }
        );
    }

    /// Locks and hides the cursor so the mouse aims, like a shooter.
    fn capture(&mut self, on: bool) {
        let Some(w) = &self.window else { return };
        if on {
            let locked = w.set_cursor_grab(CursorGrabMode::Locked).or_else(|_| w.set_cursor_grab(CursorGrabMode::Confined));
            if let Err(e) = locked {
                println!("guest: can't capture the mouse ({e})");
                return;
            }
            w.set_cursor_visible(false);
        } else {
            let _ = w.set_cursor_grab(CursorGrabMode::None);
            w.set_cursor_visible(true);
        }
        self.captured = on;
    }

    fn key(&mut self, code: KeyCode, pressed: bool) {
        if pressed {
            self.keys.insert(code);
        } else {
            self.keys.remove(&code);
            return;
        }
        match code {
            // Never quits (Cmd+Q does): Esc is a game key, too easy to hit.
            KeyCode::Escape | KeyCode::F8 => self.enter_host(),
            KeyCode::Space => self.jump = true,
            KeyCode::KeyR => self.reload = true,
            KeyCode::Tab => self.capture(!self.captured),
            KeyCode::KeyF => {
                if let Some(l) = &self.link {
                    l.bridge.request_action();
                }
            }
            KeyCode::KeyV => {
                self.walk = !self.walk;
                if self.walk {
                    // Drop in where the camera is.
                    self.walker = Some(Walker::new(self.cam.pos - Vec3::Y * walker::EYE));
                    self.pending = None;
                }
                println!("guest: {}", if self.walk { "walking" } else { "flying" });
            }
            KeyCode::KeyC => {
                let at = self.cam.pos + self.cam.flat_forward() * 3.0;
                self.place_cube(at);
            }
            KeyCode::F5 => {
                self.release();
                self.phase = Phase::WaitCamera { not_before: self.last_host + 20 };
                self.set_title("handing the camera back to Elden Ring");
            }
            KeyCode::F1 => {
                self.depth_test = !self.depth_test;
                println!("guest: depth test {}", if self.depth_test { "on" } else { "off" });
            }
            KeyCode::F2 => {
                self.test_pattern = !self.test_pattern;
                if let Some(l) = &self.link {
                    l.bridge.set_test_pattern(self.test_pattern);
                }
                println!("guest: compositor test pattern {}", if self.test_pattern { "on" } else { "off" });
            }
            KeyCode::F3 => {
                // The bridge wraps the index to the number of candidates it found (3 here).
                self.depth_index = (self.depth_index + 1) % 3;
                println!("guest: occlusion depth buffer {}", self.depth_index);
            }
            _ => {}
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        // The overlay: nothing of its own is drawn (Elden Ring composites our frames), it only
        // holds the keyboard and mouse. Hidden until there's a game to play.
        let attrs = Window::default_attributes()
            .with_title("BO3 guest")
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_visible(false)
            .with_inner_size(LogicalSize::new(1280.0, 720.0));
        #[cfg(target_os = "macos")]
        let attrs = {
            use winit::platform::macos::WindowAttributesExtMacOS;
            attrs.with_has_shadow(false)
        };
        let window = Arc::new(el.create_window(attrs).expect("window"));
        // Clicks on a fully transparent window would otherwise fall through to what's below.
        let _ = window.set_cursor_hittest(true);
        let mut renderer = Renderer::new(None);
        let root = viewmodel::default_root();
        let t0 = Instant::now();
        let stats = WeaponStats::load(&root).unwrap_or_else(|e| {
            println!("guest: weapon stats from built-in values ({e})");
            WeaponStats::default()
        });
        println!(
            "guest: {}: {:.0} rpm, {} rounds, damage {:?}, hip {}-{} deg, ADS {} deg",
            stats.name,
            60.0 / stats.fire_time,
            stats.clip_size,
            stats.damage,
            stats.hip_min,
            stats.hip_max,
            stats.ads_spread
        );
        self.combat = Combat::new(stats.clone());
        match Viewmodel::load(&root, stats) {
            Ok(vm) => {
                renderer.load_viewmodel(&vm);
                println!("guest: AP9 loaded from {} in {:.1?}", root.display(), t0.elapsed());
                self.vm = Some(vm);
            }
            Err(e) => println!("guest: no AP9 ({e}); set BO3_MODTOOLS to the mod tools download. Cube only."),
        }
        if let Ok(text) = std::fs::read_to_string(save_path()) {
            self.zombies.load_string(&text);
            self.saved = text;
            println!("guest: zombies progress restored: round {}, {} points", self.zombies.round, self.zombies.points);
        }
        self.audio = Audio::new();
        println!("guest: sound {}", if self.audio.is_some() { "on" } else { "off (no output device)" });
        self.renderer = Some(renderer);
        self.window = Some(window);
    }

    fn window_event(&mut self, _el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            // Cmd+W and friends: hand control to Elden Ring rather than closing.
            WindowEvent::CloseRequested => self.enter_host(),
            WindowEvent::Resized(size) => {
                if let Some(r) = &mut self.renderer {
                    r.resize_preview(size.width, size.height);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    if !event.repeat {
                        self.key(code, event.state == ElementState::Pressed);
                    }
                }
            }
            WindowEvent::MouseInput { state, button: MouseButton::Right, .. } => {
                // Captured: right click aims. Not captured: right-drag looks around.
                let down = state == ElementState::Pressed;
                self.aim = down;
                self.looking = down && !self.captured;
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                self.trigger = state == ElementState::Pressed;
            }
            WindowEvent::Focused(false) => {
                self.keys.clear();
                self.looking = false;
                self.trigger = false;
                self.aim = false;
                self.capture(false);
            }
            WindowEvent::Focused(true) => {
                if self.overlay_on && !self.host_mode {
                    self.capture(true);
                }
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _el: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta } = event {
            if self.looking || self.captured {
                self.cam.look(delta.0 as f32, delta.1 as f32);
            }
        }
    }

    fn exiting(&mut self, _el: &ActiveEventLoop) {
        self.capture(false);
        self.release();
        self.save_progress();
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        self.tick();
        // Elden Ring presents at most every ~16 ms; checking every millisecond is plenty.
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The compositor's `linMc` (er-bridge/src/compositor.cpp): OpenGL window depth -> metres.
    fn lin_mc(d: f32, near: f32, far: f32) -> f32 {
        let z = d * 2.0 - 1.0;
        2.0 * near * far / (far + near - z * (far - near))
    }

    #[test]
    fn our_depth_linearises_like_the_compositor_expects() {
        let proj = perspective(48f32.to_radians(), 1.6, NEAR, FAR);
        for dist in [0.5f32, 2.0, 10.0, 100.0, 900.0] {
            let clip = proj * glam::Vec4::new(0.3, -0.2, dist, 1.0);
            let depth = clip.z / clip.w;
            let back = lin_mc(depth, NEAR, FAR);
            assert!((back - dist).abs() / dist < 1e-3, "{dist} m came back as {back} m");
        }
    }

    #[test]
    fn hunter_yaw_faces_the_camera_direction() {
        // The bridge turns hunterYawDeg y into forward (-sin y, 0, -cos y).
        for yaw in [0.0f32, 0.7, -2.1, 3.0] {
            let y = (yaw + std::f32::consts::PI).to_degrees().to_radians();
            let fwd = Vec3::new(-y.sin(), 0.0, -y.cos());
            let cam = FlyCam { pos: Vec3::ZERO, yaw, pitch: 0.0 };
            assert!((fwd - cam.flat_forward()).length() < 1e-5);
        }
    }
}
