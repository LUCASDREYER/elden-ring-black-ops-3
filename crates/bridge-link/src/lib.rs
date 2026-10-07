//! The guest side of the Elden Ring bridge's shared memory.
//!
//! Mirrors `elden-ring/er-bridge/include/bridge_protocol.h` from
//! `justbustin/minecraft-crossover-bridge` at commit `d1723873ec`. That header is the
//! authority; every offset below is copied from it. The DLL inside Elden Ring maps the same
//! two files through Wine (`Z:\tmp\ermc\...`), so both processes share the pages.
//!
//! - `bridge.shm` (8 MB): header, game state (written by Elden Ring), control (written by us).
//! - `frames.shm` (~110 MB): our rendered frames, three slots of up to 1920 x 1200 with world
//!   colour, world depth, GUI and hand layers.
//!
//! Blocks written while the other side may read use a seqlock: odd while writing.

use std::fs::OpenOptions;
use std::io;
use std::ptr;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering, fence};
use std::time::{SystemTime, UNIX_EPOCH};

use memmap2::MmapMut;

pub const SHM_PATH: &str = "/tmp/ermc/bridge.shm";
pub const FRAMES_PATH: &str = "/tmp/ermc/frames.shm";

const MAGIC: u32 = 0x434D_484D; // "MHMC"
const SHM_SIZE: usize = 8 * 1024 * 1024;

// ErmcHeader
const H_HOST_HEARTBEAT: usize = 0x10;
const H_GUEST_HEARTBEAT: usize = 0x18;
const H_GUEST_PID: usize = 0x24;
const H_GUEST_START_MS: usize = 0x30;
const H_DEBUG_FLAGS: usize = 0x64;

const OFF_STATE: usize = 0x100;
const STATE_SIZE: usize = 0x100;
const OFF_CONTROL: usize = 0x800;

/// `ErmcGameState.flags`
pub mod state {
    pub const CAMERA_VALID: u32 = 1 << 0;
    pub const PLAYER_VALID: u32 = 1 << 1;
    pub const WINDOW_VALID: u32 = 1 << 2;
    /// Elden Ring rendered this frame with our camera.
    pub const CAM_OVERRIDDEN: u32 = 1 << 3;
    pub const MATRICES_VALID: u32 = 1 << 4;
    pub const WINDOW_FOCUSED: u32 = 1 << 5;
    /// Elden Ring is drawing our frames into its own.
    pub const COMPOSITING: u32 = 1 << 6;
    pub const PLAYER_DEAD: u32 = 1 << 7;
    /// In a session but no usable Tarnished (loading screen, death): draw nothing.
    pub const HOST_BUSY: u32 = 1 << 8;
}

/// `ErmcControl.flags`
pub mod ctrl {
    pub const OVERRIDE_CAMERA: u32 = 1 << 0;
    pub const MOVE_HUNTER: u32 = 1 << 1;
    pub const HIDE_HUNTER: u32 = 1 << 2;
    pub const COMPOSITE: u32 = 1 << 4;
    pub const NO_DEPTH_TEST: u32 = 1 << 5;
    pub const DEBUG_DEPTH: u32 = 1 << 6;
    pub const NO_RELIGHT: u32 = 1 << 7;
}

/// `ErmcGameState`: Elden Ring's view of the world, in its own units (metres, left-handed:
/// +X right, +Y up, +Z forward).
#[derive(Clone, Copy, Debug, Default)]
pub struct GameState {
    pub flags: u32,
    pub frame: u64,
    pub cam_pos: [f32; 3],
    pub cam_target: [f32; 3],
    pub cam_up: [f32; 3],
    pub fov_y_deg: f32,
    pub near: f32,
    pub far: f32,
    pub aspect: f32,
    pub units_per_meter: f32,
    pub player_pos: [f32; 3],
    pub player_quat: [f32; 4],
    /// Game client area in screen coordinates: x, y, w, h.
    pub win: [i32; 4],
    /// Swapchain back buffer size.
    pub bb: [u32; 2],
    pub stage_id: u32,
}

impl GameState {
    pub fn has(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }
}

/// `ErmcControl`: what we ask of Elden Ring each frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Control {
    pub flags: u32,
    /// `mcFrame`: the id of the frame rendered with this pose. Elden Ring composites the
    /// `frames.shm` slot whose `poseId` matches the pose it rendered with.
    pub pose_id: u64,
    pub cam_pos: [f32; 3],
    pub cam_target: [f32; 3],
    pub cam_up: [f32; 3],
    pub fov_y_deg: f32,
    pub hunter_pos: [f32; 3],
    /// Game frames between applying a pose and presenting it (the Minecraft mod uses 1).
    pub pose_lag: u32,
    /// Which of Elden Ring's depth buffers is the scene depth (0/1).
    pub depth_index: u32,
    pub light_gain: f32,
    pub light_min: f32,
    pub fog_strength: f32,
    pub hunter_yaw_deg: f32,
}

fn other(msg: impl Into<String>) -> io::Error {
    io::Error::other(msg.into())
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// `bridge.shm`, created by the DLL when Elden Ring starts through `launch_er.sh`.
pub struct Bridge {
    map: MmapMut,
}

impl Bridge {
    pub fn open() -> io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(SHM_PATH)?;
        if file.metadata()?.len() < SHM_SIZE as u64 {
            return Err(other("bridge.shm is too small: the Elden Ring bridge hasn't initialised it yet"));
        }
        let map = unsafe { MmapMut::map_mut(&file)? };
        let bridge = Bridge { map };
        let magic = bridge.atomic_u32(0).load(Ordering::Acquire);
        if magic != MAGIC {
            return Err(other(format!("bridge.shm has magic {magic:#x}, expected {MAGIC:#x}")));
        }
        Ok(bridge)
    }

    fn atomic_u32(&self, off: usize) -> &AtomicU32 {
        assert!(off % 4 == 0 && off + 4 <= self.map.len());
        unsafe { &*(self.map.as_ptr().add(off) as *const AtomicU32) }
    }

    fn atomic_u64(&self, off: usize) -> &AtomicU64 {
        assert!(off % 8 == 0 && off + 8 <= self.map.len());
        unsafe { &*(self.map.as_ptr().add(off) as *const AtomicU64) }
    }

    /// Incremented by Elden Ring once per presented frame.
    pub fn host_heartbeat(&self) -> u64 {
        self.atomic_u64(H_HOST_HEARTBEAT).load(Ordering::Acquire)
    }

    /// Tells the bridge tools (`erctl.py fps`) that a guest is attached.
    pub fn announce(&self) {
        self.atomic_u32(H_GUEST_PID).store(std::process::id(), Ordering::Release);
        self.atomic_u64(H_GUEST_START_MS).store(now_ms(), Ordering::Release);
    }

    /// Call once per guest frame.
    pub fn beat(&self) {
        self.atomic_u64(H_GUEST_HEARTBEAT).fetch_add(1, Ordering::AcqRel);
    }

    /// `debugFlags` bit 0 makes the compositor draw a test pattern.
    pub fn set_test_pattern(&self, on: bool) {
        let f = self.atomic_u32(H_DEBUG_FLAGS);
        if on {
            f.fetch_or(1, Ordering::AcqRel);
        } else {
            f.fetch_and(!1, Ordering::AcqRel);
        }
    }

    /// A consistent snapshot of the game state, or `None` if Elden Ring kept writing it.
    pub fn state(&self) -> Option<GameState> {
        let seq = self.atomic_u32(OFF_STATE);
        for _ in 0..256 {
            let s0 = seq.load(Ordering::Acquire);
            if s0 & 1 != 0 {
                std::hint::spin_loop();
                continue;
            }
            let mut raw = [0u8; STATE_SIZE];
            unsafe {
                let src = self.map.as_ptr().add(OFF_STATE);
                for (i, b) in raw.iter_mut().enumerate() {
                    *b = ptr::read_volatile(src.add(i));
                }
            }
            fence(Ordering::Acquire);
            if seq.load(Ordering::Relaxed) == s0 {
                return Some(parse_state(&raw));
            }
        }
        None
    }

    pub fn write_control(&self, c: &Control) {
        let seq = self.atomic_u32(OFF_CONTROL);
        let mut s = seq.load(Ordering::Relaxed);
        if s & 1 != 0 {
            s = s.wrapping_add(1);
        }
        seq.store(s.wrapping_add(1), Ordering::Relaxed);
        fence(Ordering::Release);

        let mut raw = [0u8; 0x5C];
        put_u32(&mut raw, 0x04, c.flags);
        raw[0x08..0x10].copy_from_slice(&c.pose_id.to_le_bytes());
        put_f32s(&mut raw, 0x10, &c.cam_pos);
        put_f32s(&mut raw, 0x1C, &c.cam_target);
        put_f32s(&mut raw, 0x28, &c.cam_up);
        put_f32s(&mut raw, 0x34, &[c.fov_y_deg]);
        put_f32s(&mut raw, 0x38, &c.hunter_pos);
        put_u32(&mut raw, 0x44, c.pose_lag);
        put_u32(&mut raw, 0x48, c.depth_index);
        put_f32s(&mut raw, 0x4C, &[c.light_gain, c.light_min, c.fog_strength, c.hunter_yaw_deg]);
        unsafe {
            let dst = self.map.as_ptr().add(OFF_CONTROL) as *mut u8;
            for (i, b) in raw.iter().enumerate().skip(4) {
                ptr::write_volatile(dst.add(i), *b);
            }
        }
        seq.store(s.wrapping_add(2), Ordering::Release);
    }
}

// ErmcRayHeader at 0x100000: rays at +0x20 (24 bytes each), hits after all 8192 rays.
const OFF_RAYS: usize = 0x10_0000;
const MAX_RAYS: usize = 8192;
const R_REQ_SEQ: usize = OFF_RAYS;
const R_RESP_SEQ: usize = OFF_RAYS + 0x04;
const R_COUNT: usize = OFF_RAYS + 0x08;
const R_FLAGS: usize = OFF_RAYS + 0x0C;
const R_RAYS: usize = OFF_RAYS + 0x20;
const R_HITS: usize = R_RAYS + MAX_RAYS * 24;

/// Where a ray met Elden Ring's collision.
#[derive(Clone, Copy, Debug)]
pub struct RayHit {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    /// Collision attribute bits of the surface.
    pub attr: u32,
}

impl Bridge {
    /// True when no ray batch is in flight (the last one was answered, or none was sent).
    pub fn rays_idle(&self) -> bool {
        self.atomic_u32(R_RESP_SEQ).load(Ordering::Acquire) == self.atomic_u32(R_REQ_SEQ).load(Ordering::Acquire)
    }

    /// Sends segments `start -> end` to be cast against Elden Ring's own collision on its game
    /// thread (it skips characters). Returns the batch's sequence number, or `None` if the
    /// previous batch is still in flight. Answers usually arrive by the next game frame.
    pub fn submit_rays(&self, rays: &[([f32; 3], [f32; 3])]) -> Option<u32> {
        assert!(rays.len() <= MAX_RAYS);
        if !self.rays_idle() {
            return None;
        }
        let mut raw = vec![0u8; rays.len() * 24];
        for (i, (s, e)) in rays.iter().enumerate() {
            put_f32s(&mut raw, i * 24, s);
            put_f32s(&mut raw, i * 24 + 12, e);
        }
        unsafe {
            let dst = self.map.as_ptr().add(R_RAYS) as *mut u8;
            for (i, b) in raw.iter().enumerate() {
                ptr::write_volatile(dst.add(i), *b);
            }
        }
        self.atomic_u32(R_COUNT).store(rays.len() as u32, Ordering::Relaxed);
        self.atomic_u32(R_FLAGS).store(0, Ordering::Relaxed);
        let req = self.atomic_u32(R_REQ_SEQ);
        let seq = req.load(Ordering::Relaxed).wrapping_add(1);
        req.store(seq, Ordering::Release);
        Some(seq)
    }

    /// True once batch `seq` has been answered.
    pub fn rays_done(&self, seq: u32) -> bool {
        self.atomic_u32(R_RESP_SEQ).load(Ordering::Acquire) == seq
    }

    /// The answers to the last answered batch, `count` of them.
    pub fn read_hits(&self, count: usize) -> Vec<Option<RayHit>> {
        let mut hits = Vec::with_capacity(count);
        for i in 0..count.min(MAX_RAYS) {
            let mut h = [0u8; 32];
            unsafe {
                let src = self.map.as_ptr().add(R_HITS + i * 32);
                for (k, b) in h.iter_mut().enumerate() {
                    *b = ptr::read_volatile(src.add(k));
                }
            }
            hits.push((get_u32(&h, 24) != 0).then(|| RayHit {
                pos: get_f32s(&h, 0),
                normal: get_f32s(&h, 12),
                attr: get_u32(&h, 28),
            }));
        }
        hits
    }

    /// [`submit_rays`](Self::submit_rays) and wait for the answer; `None` on timeout.
    pub fn raycast(&self, rays: &[([f32; 3], [f32; 3])], timeout: std::time::Duration) -> Option<Vec<Option<RayHit>>> {
        let deadline = std::time::Instant::now() + timeout;
        let until = |done: &dyn Fn() -> bool| {
            while !done() {
                if std::time::Instant::now() > deadline {
                    return false;
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            true
        };
        // A batch still in flight (from an earlier caller) must finish first.
        if !until(&|| self.rays_idle()) {
            return None;
        }
        let seq = self.submit_rays(rays)?;
        if !until(&|| self.rays_done(seq)) {
            return None;
        }
        Some(self.read_hits(rays.len()))
    }

    // -- switching the keyboard and mouse between the games (F8) ----------------------------

    /// +1 each time F8 is pressed while Elden Ring has focus: the guest should take over again.
    pub fn switch_requests(&self) -> u32 {
        self.atomic_u32(H_SWITCH_REQ).load(Ordering::Acquire)
    }

    /// Asks Elden Ring to bring its window to the front (the guest handed it control).
    pub fn request_host_focus(&self) {
        self.atomic_u32(H_FOCUS_REQ).fetch_add(1, Ordering::AcqRel);
    }

    // -- Elden Ring's own "action" (doors, levers, items, graces) ---------------------------

    /// Asks Elden Ring to perform the action it currently offers (the prompt).
    pub fn request_action(&self) {
        self.atomic_u32(H_ACTION_REQ).fetch_add(1, Ordering::AcqRel);
    }

    /// The action on offer ("Open", "Pick up"...), empty if none.
    pub fn prompt(&self) -> String {
        let mut raw = [0u8; 64];
        unsafe {
            let src = self.map.as_ptr().add(H_PROMPT);
            for (i, b) in raw.iter_mut().enumerate() {
                *b = ptr::read_volatile(src.add(i));
            }
        }
        let len = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
        String::from_utf8_lossy(&raw[..len]).into_owned()
    }

    // -- life ------------------------------------------------------------------------------

    /// +1 each time the Tarnished is usable again somewhere the guest didn't put it (after a
    /// load, a respawn, a warp): the guest moves its player there before driving again.
    pub fn host_life(&self) -> u32 {
        self.atomic_u32(H_HOST_LIFE).load(Ordering::Acquire)
    }

    /// +1 each time the standing-in Tarnished died in Elden Ring (a kill plane...).
    pub fn host_deaths(&self) -> u32 {
        self.atomic_u32(H_HOST_DEATHS).load(Ordering::Acquire)
    }

    /// The guest's player died: the standing-in Tarnished dies too, and Elden Ring respawns it
    /// at a Site of Grace.
    pub fn guest_died(&self) {
        self.atomic_u32(H_GUEST_DEATHS).fetch_add(1, Ordering::AcqRel);
    }

    /// Hits the standing-in Tarnished took, as running totals.
    pub fn hunter_events(&self) -> Option<HunterEvents> {
        let raw = self.read_seqlocked::<0x30>(OFF_HUNTER)?;
        Some(HunterEvents {
            hit_count: get_u32(&raw, 0x04),
            total_damage: get_f32s::<1>(&raw, 0x08)[0],
            last_damage: get_f32s::<1>(&raw, 0x0C)[0],
            last_hit_from: get_f32s(&raw, 0x10),
            max_hp: get_f32s::<1>(&raw, 0x1C)[0],
            last_hit_frame: u64::from_le_bytes(raw[0x20..0x28].try_into().unwrap()),
            last_hit_kind: get_u32(&raw, 0x28),
            total_status_damage: get_f32s::<1>(&raw, 0x2C)[0],
        })
    }

    fn read_seqlocked<const N: usize>(&self, off: usize) -> Option<[u8; N]> {
        let seq = self.atomic_u32(off);
        for _ in 0..256 {
            let s0 = seq.load(Ordering::Acquire);
            if s0 & 1 != 0 {
                std::hint::spin_loop();
                continue;
            }
            let mut raw = [0u8; N];
            unsafe {
                let src = self.map.as_ptr().add(off);
                for (i, b) in raw.iter_mut().enumerate() {
                    *b = ptr::read_volatile(src.add(i));
                }
            }
            fence(Ordering::Acquire);
            if seq.load(Ordering::Relaxed) == s0 {
                return Some(raw);
            }
        }
        None
    }

    // -- fights ----------------------------------------------------------------------------

    /// The hittable characters Elden Ring published this frame. False if the table was being
    /// rewritten throughout (try again next frame).
    pub fn entities(&self, out: &mut Vec<Entity>) -> bool {
        let seq = self.atomic_u32(OFF_ENTITIES);
        for _ in 0..64 {
            let s0 = seq.load(Ordering::Acquire);
            if s0 & 1 != 0 {
                std::hint::spin_loop();
                continue;
            }
            let count = (self.atomic_u32(OFF_ENTITIES + 4).load(Ordering::Relaxed) as usize).min(MAX_ENTITIES);
            let mut raw = vec![0u8; count * ENTITY_SIZE];
            unsafe {
                let src = self.map.as_ptr().add(OFF_ENTITIES + 0x10);
                for (i, b) in raw.iter_mut().enumerate() {
                    *b = ptr::read_volatile(src.add(i));
                }
            }
            fence(Ordering::Acquire);
            if seq.load(Ordering::Relaxed) != s0 {
                continue;
            }
            out.clear();
            out.extend(raw.chunks_exact(ENTITY_SIZE).map(parse_entity));
            return true;
        }
        false
    }

    /// Queues a hit for Elden Ring to apply on its game thread.
    pub fn push_damage(&self, d: &Damage) {
        let write = self.atomic_u32(OFF_DAMAGE);
        let w = write.load(Ordering::Relaxed);
        let mut raw = [0u8; 0x20];
        raw[0..8].copy_from_slice(&d.id.to_le_bytes());
        put_f32s(&mut raw, 0x08, &[d.amount]);
        put_f32s(&mut raw, 0x0C, &d.hit_pos);
        put_u32(&mut raw, 0x18, d.flags);
        unsafe {
            let dst = self.map.as_ptr().add(OFF_DAMAGE + 0x10 + (w as usize % DAMAGE_RING) * 0x20) as *mut u8;
            for (i, b) in raw.iter().enumerate() {
                ptr::write_volatile(dst.add(i), *b);
            }
        }
        write.store(w.wrapping_add(1), Ordering::Release);
    }
}

// ErmcHeader life counters, ErmcHunterEvents, entity table and damage queue.
const H_SWITCH_REQ: usize = 0x48;
const H_FOCUS_REQ: usize = 0x4C;
const H_HOST_LIFE: usize = 0x58;
const H_ACTION_REQ: usize = 0x70;
const H_PROMPT: usize = 0x80;
const H_GUEST_DEATHS: usize = 0x5C;
const H_HOST_DEATHS: usize = 0x60;
const OFF_HUNTER: usize = 0xA00;
const OFF_ENTITIES: usize = 0x20_0000;
const MAX_ENTITIES: usize = 256;
const ENTITY_SIZE: usize = 0x80;
const OFF_DAMAGE: usize = 0x28_0000;
const DAMAGE_RING: usize = 256;

#[derive(Clone, Copy, Debug, Default)]
pub struct HunterEvents {
    pub hit_count: u32,
    /// Elden Ring HP lost, summed.
    pub total_damage: f32,
    pub last_damage: f32,
    /// Where the attacker stood at the last hit.
    pub last_hit_from: [f32; 3],
    pub max_hp: f32,
    pub last_hit_frame: u64,
    pub last_hit_kind: u32,
    pub total_status_damage: f32,
}

/// `ErmcEntity.kind` and `.flags`
pub mod ent {
    pub const LARGE_MONSTER: u32 = 1;
    pub const SMALL_MONSTER: u32 = 2;
    /// Not hostile (merchants, spirits...).
    pub const OTHER: u32 = 3;
    pub const DEAD: u32 = 1 << 0;
    /// The box is world-aligned (ignore the rotation).
    pub const WORLD_BOX: u32 = 1 << 1;
}

#[derive(Clone, Debug, Default)]
pub struct Entity {
    /// Stable while the character lives.
    pub id: u64,
    pub kind: u32,
    pub npc_param: u32,
    /// Feet, in the same frame as the game state.
    pub pos: [f32; 3],
    pub quat: [f32; 4],
    pub box_center: [f32; 3],
    pub box_half: [f32; 3],
    pub hp: f32,
    pub max_hp: f32,
    pub flags: u32,
    pub name: String,
}

impl Entity {
    pub fn hostile(&self) -> bool {
        self.kind == ent::LARGE_MONSTER || self.kind == ent::SMALL_MONSTER
    }
    pub fn dead(&self) -> bool {
        self.flags & ent::DEAD != 0
    }
}

fn parse_entity(b: &[u8]) -> Entity {
    let name = &b[0x50..0x80];
    let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    Entity {
        id: u64::from_le_bytes(b[0..8].try_into().unwrap()),
        kind: get_u32(b, 0x08),
        npc_param: get_u32(b, 0x0C),
        pos: get_f32s(b, 0x10),
        quat: get_f32s(b, 0x1C),
        box_center: get_f32s(b, 0x2C),
        box_half: get_f32s(b, 0x38),
        hp: get_f32s::<1>(b, 0x44)[0],
        max_hp: get_f32s::<1>(b, 0x48)[0],
        flags: get_u32(b, 0x4C),
        name: String::from_utf8_lossy(&name[..len]).into_owned(),
    }
}

/// `ErmcDamage.flags`
pub mod dmg {
    pub const CRITICAL: u32 = 1 << 0;
    pub const OUTWARD: u32 = 1 << 1;
    pub const NOT_BY_PLAYER: u32 = 1 << 2;
}

/// A hit for Elden Ring to apply. `amount` is in the bridge's damage points: Elden Ring
/// converts it to HP as `amount * maxHp / clamp(20 * sqrt(maxHp / 100), 10, 300)`, so tougher
/// characters take more hits.
#[derive(Clone, Copy, Debug, Default)]
pub struct Damage {
    /// Target [`Entity::id`]; 0 strikes the world at `hit_pos` (breaks crates and pots).
    pub id: u64,
    pub amount: f32,
    pub hit_pos: [f32; 3],
    pub flags: u32,
}

fn get_u32(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().unwrap())
}

fn get_f32s<const N: usize>(b: &[u8], off: usize) -> [f32; N] {
    std::array::from_fn(|i| f32::from_bits(get_u32(b, off + i * 4)))
}

fn put_u32(b: &mut [u8], off: usize, v: u32) {
    b[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

fn put_f32s(b: &mut [u8], off: usize, v: &[f32]) {
    for (i, x) in v.iter().enumerate() {
        put_u32(b, off + i * 4, x.to_bits());
    }
}

fn parse_state(b: &[u8]) -> GameState {
    GameState {
        flags: get_u32(b, 0x04),
        frame: u64::from_le_bytes(b[0x08..0x10].try_into().unwrap()),
        cam_pos: get_f32s(b, 0x10),
        cam_target: get_f32s(b, 0x1C),
        cam_up: get_f32s(b, 0x28),
        fov_y_deg: get_f32s::<1>(b, 0x34)[0],
        near: get_f32s::<1>(b, 0x38)[0],
        far: get_f32s::<1>(b, 0x3C)[0],
        aspect: get_f32s::<1>(b, 0x40)[0],
        units_per_meter: get_f32s::<1>(b, 0x44)[0],
        player_pos: get_f32s(b, 0x48),
        player_quat: get_f32s(b, 0x54),
        win: std::array::from_fn(|i| get_u32(b, 0x64 + i * 4) as i32),
        bb: [get_u32(b, 0x74), get_u32(b, 0x78)],
        stage_id: get_u32(b, 0x7C),
    }
}

// ---------------------------------------------------------------------------------------
// frames.shm

pub const FRAME_MAX_W: u32 = 1920;
pub const FRAME_MAX_H: u32 = 1200;
const FRAMES_MAGIC: u32 = 0x524D_484D; // "MHMR"
const FRAMES_VERSION: u32 = 2;
const SLOTS: u64 = 3;
const FRAME_HDR: usize = 0x100;
const SLOT_SIZE: usize = FRAME_HDR + (FRAME_MAX_W * FRAME_MAX_H * 16) as usize;
const FRAMES_FILE_SIZE: u64 = 0x1000 + SLOTS * SLOT_SIZE as u64;

/// `ErmcFrameHeader.flags`
const FRAME_WORLD: u32 = 1 << 0;
const FRAME_GUI: u32 = 1 << 1;
const FRAME_HAND: u32 = 1 << 2;

/// One rendered frame, as rows straight from the GPU (top-down, possibly padded).
pub struct FrameOut<'a> {
    /// Also the pose id: publish the camera that rendered it with the same number.
    pub frame_id: u64,
    pub width: u32,
    pub height: u32,
    /// Projection near and far planes. Depth must be `[0, 1]` from a standard (not reversed)
    /// perspective with these planes; that equals OpenGL window depth, which the DLL expects.
    pub near: f32,
    pub far: f32,
    pub fov_y_deg: f32,
    /// BGRA8, premultiplied alpha.
    pub world: &'a [u8],
    /// f32 depth.
    pub depth: &'a [u8],
    /// BGRA8, lit like the world but never occluded. `None` leaves the hand layer out.
    pub hand: Option<&'a [u8]>,
    /// BGRA8 HUD, drawn as is on top. `None` leaves it empty.
    pub gui: Option<&'a [u8]>,
    /// Bytes between rows of `gui` (it's usually built on the CPU, unpadded).
    pub gui_stride: usize,
    /// Bytes between rows in the three buffers above.
    pub row_stride: usize,
}

pub struct Frames {
    map: MmapMut,
}

impl Frames {
    pub fn open() -> io::Result<Self> {
        std::fs::create_dir_all("/tmp/ermc")?;
        let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(FRAMES_PATH)?;
        if file.metadata()?.len() < FRAMES_FILE_SIZE {
            file.set_len(FRAMES_FILE_SIZE)?;
        }
        let map = unsafe { MmapMut::map_mut(&file)? };
        let frames = Frames { map };
        frames.atomic_u32(4).store(FRAMES_VERSION, Ordering::Relaxed);
        frames.atomic_u32(0).store(FRAMES_MAGIC, Ordering::Release);
        Ok(frames)
    }

    fn atomic_u32(&self, off: usize) -> &AtomicU32 {
        assert!(off % 4 == 0 && off + 4 <= self.map.len());
        unsafe { &*(self.map.as_ptr().add(off) as *const AtomicU32) }
    }

    fn atomic_u64(&self, off: usize) -> &AtomicU64 {
        assert!(off % 8 == 0 && off + 8 <= self.map.len());
        unsafe { &*(self.map.as_ptr().add(off) as *const AtomicU64) }
    }

    /// Writes a slot, rows flipped bottom-up as the DLL expects. Publish the frame's camera
    /// pose with [`Bridge::write_control`] only after this returns.
    pub fn publish(&mut self, f: &FrameOut) {
        assert!(f.width <= FRAME_MAX_W && f.height <= FRAME_MAX_H);
        let (w, h) = (f.width as usize, f.height as usize);
        let row = w * 4;
        let layer = row * h;
        let slot = (f.frame_id % SLOTS) as usize;
        let base = 0x1000 + slot * SLOT_SIZE;

        // Odd while writing: the DLL skips the slot, and a copy it started sees the change.
        // A raw pointer, so the map can be written while the sequence word is held.
        let seq_ptr = self.atomic_u32(base) as *const AtomicU32;
        let seq = unsafe { &*seq_ptr };
        let s = seq.load(Ordering::Relaxed) | 1;
        seq.store(s, Ordering::Relaxed);
        fence(Ordering::Release);

        let data = base + FRAME_HDR;
        let copy_flipped = |map: &mut MmapMut, dst: usize, src: &[u8]| {
            for y in 0..h {
                let s0 = y * f.row_stride;
                let d0 = dst + (h - 1 - y) * row;
                map[d0..d0 + row].copy_from_slice(&src[s0..s0 + row]);
            }
        };
        copy_flipped(&mut self.map, data, f.world);
        copy_flipped(&mut self.map, data + layer, f.depth);
        match f.gui {
            Some(gui) => {
                for y in 0..h {
                    let s0 = y * f.gui_stride;
                    let d0 = data + 2 * layer + (h - 1 - y) * row;
                    self.map[d0..d0 + row].copy_from_slice(&gui[s0..s0 + row]);
                }
            }
            None => self.map[data + 2 * layer..data + 3 * layer].fill(0),
        }
        if let Some(hand) = f.hand {
            copy_flipped(&mut self.map, data + 3 * layer, hand);
        }

        let mut hdr = [0u8; 0x30];
        put_u32(&mut hdr, 0x04, f.width);
        put_u32(&mut hdr, 0x08, f.height);
        put_u32(&mut hdr, 0x0C, FRAME_WORLD | FRAME_GUI | if f.hand.is_some() { FRAME_HAND } else { 0 });
        hdr[0x10..0x18].copy_from_slice(&f.frame_id.to_le_bytes());
        hdr[0x18..0x20].copy_from_slice(&f.frame_id.to_le_bytes());
        put_f32s(&mut hdr, 0x20, &[f.near, f.far, f.fov_y_deg, f.width as f32 / f.height as f32]);
        self.map[base + 4..base + 0x30].copy_from_slice(&hdr[4..]);

        fence(Ordering::Release);
        seq.store(s.wrapping_add(1), Ordering::Release);
        self.atomic_u32(0x08).store(slot as u32, Ordering::Release);
        self.atomic_u64(0x10).store(f.frame_id, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_header() {
        // static_asserts from bridge_protocol.h
        assert_eq!(SLOT_SIZE, 0x100 + 1920 * 1200 * 16);
        assert_eq!(FRAMES_FILE_SIZE, 0x1000 + 3 * SLOT_SIZE as u64);
        assert_eq!(OFF_CONTROL - OFF_STATE, 0x700);
    }

    #[test]
    fn entity_fields_at_header_offsets() {
        let mut b = [0u8; ENTITY_SIZE];
        b[0..8].copy_from_slice(&0xABCDu64.to_le_bytes());
        put_u32(&mut b, 0x08, ent::SMALL_MONSTER);
        put_f32s(&mut b, 0x2C, &[1.0, 2.0, 3.0]);
        put_f32s(&mut b, 0x38, &[0.4, 0.9, 0.4]);
        put_f32s(&mut b, 0x44, &[120.0, 300.0]);
        put_u32(&mut b, 0x4C, ent::WORLD_BOX);
        b[0x50..0x55].copy_from_slice(b"c3000");
        let e = parse_entity(&b);
        assert_eq!(e.id, 0xABCD);
        assert!(e.hostile() && !e.dead());
        assert_eq!((e.box_center, e.box_half, e.hp, e.max_hp), ([1.0, 2.0, 3.0], [0.4, 0.9, 0.4], 120.0, 300.0));
        assert_eq!(e.name, "c3000");
    }

    #[test]
    fn state_roundtrip_fields() {
        let mut raw = [0u8; STATE_SIZE];
        put_u32(&mut raw, 0x04, state::CAMERA_VALID | state::PLAYER_VALID);
        put_f32s(&mut raw, 0x10, &[1.0, 2.0, 3.0]);
        put_f32s(&mut raw, 0x34, &[48.0]);
        put_u32(&mut raw, 0x64, (-5i32) as u32);
        put_u32(&mut raw, 0x74, 1280);
        put_u32(&mut raw, 0x78, 800);
        let s = parse_state(&raw);
        assert!(s.has(state::CAMERA_VALID) && s.has(state::PLAYER_VALID));
        assert_eq!(s.cam_pos, [1.0, 2.0, 3.0]);
        assert_eq!(s.fov_y_deg, 48.0);
        assert_eq!(s.win[0], -5);
        assert_eq!(s.bb, [1280, 800]);
    }
}
