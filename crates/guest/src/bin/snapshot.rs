//! Renders the first-person weapon offscreen to PNG files, through the same renderer and
//! hand-layer readback the guest sends to Elden Ring. For checking the view model without
//! the game or a screen.
//!
//!   snapshot <out_dir>

use std::path::Path;
use std::time::Instant;

use glam::{Mat4, Vec3};
use guest::render::{HandFrame, Renderer};
use guest::viewmodel::{self, Action, Viewmodel};

const W: u32 = 1280;
const H: u32 = 720;

fn main() {
    let out = std::env::args().nth(1).expect("usage: snapshot <out_dir>");
    std::fs::create_dir_all(&out).expect("out dir");
    let root = viewmodel::default_root();
    let t0 = Instant::now();
    let stats = guest::stats::WeaponStats::load(&root).unwrap_or_default();
    println!("sway from the weapon file: {:?}", stats.sway);
    let mut vm = Viewmodel::load(&root, stats).unwrap_or_else(|e| panic!("loading from {}: {e}", root.display()));
    println!("loaded view model in {:.2?}", t0.elapsed());
    for p in &vm.parts {
        let textured = p.materials.iter().filter(|m| m.texture.is_some()).count();
        println!("  {}: {} triangles, {} bones, {}/{} materials textured", p.name, p.indices.len() / 3, p.bone_count(), textured, p.materials.len());
        if std::env::var_os("SNAPSHOT_MATERIALS").is_some() {
            for m in &p.materials {
                println!("      {:<45} {}", m.name, m.texture.as_ref().map(|t| format!("{}x{}", t.width, t.height)).unwrap_or("-".into()));
            }
        }
    }
    let mut r = Renderer::new(None);
    r.load_viewmodel(&vm);

    let reload = vm.clip_duration(Action::Reload);
    let shots = [
        ("1_idle", Action::Idle, 0.0),
        ("2_fire", Action::Fire, 0.04),
        ("3_raise", Action::Raise, 0.25),
        ("4_reload_clip_out", Action::Reload, 19.0 / 30.0),
        ("5_reload_mid", Action::Reload, reload * 0.45),
        ("6_reload_clip_in", Action::Reload, 44.0 / 30.0),
        ("a_ads", Action::Idle, 0.0),
        ("a_ads_fire_flash", Action::FireAds, 0.03),
    ];
    // A debug camera off to the right of the rig, in view space, to see what the first-person
    // view hides.
    let side = {
        let tv = viewmodel::to_view();
        let eye = tv.transform_point3(Vec3::new(4.0, -34.0, -2.0));
        let target = tv.transform_point3(Vec3::new(11.0, -2.0, -9.0));
        guest::perspective(70f32.to_radians(), W as f32 / H as f32, 0.01, 10.0)
            * glam::camera::lh::view::look_at_mat4(eye, target, Vec3::Y)
            * tv
    };
    let shots: Vec<(&str, Action, f32, bool)> = shots
        .iter()
        .map(|&(n, a, t)| (n, a, t, false))
        .chain([("7_side_idle", Action::Idle, 0.0, true), ("8_side_reload_clip_out", Action::Reload, 19.0 / 30.0, true), ("9_side_reload_mid", Action::Reload, 30.0 / 30.0, true)])
        .collect();
    let mut skins = Vec::new();
    for (name, action, t, side_view) in shots {
        vm.set(action, t);
        let ads = name.starts_with("a_");
        vm.set_ads(if ads { 1.0 } else { 0.0 });
        vm.skin(&mut skins);
        let zoom = if ads { viewmodel::ADS_ZOOM } else { 1.0 };
        let proj = guest::hand_proj(W as f32 / H as f32, zoom);
        let view_proj = if side_view { side } else { proj * viewmodel::to_view() };
        let flash = name.contains("flash").then(|| guest::render::MuzzleFlash {
            center: viewmodel::to_view().transform_point3(vm.muzzle(&skins).unwrap()),
            half_size: 0.07,
            intensity: 1.0,
            rotation: 0.4,
        });
        let hand = HandFrame { view_proj, to_view: viewmodel::to_view(), proj, skins: &skins, flash };
        // The world layer is empty here: the cube sits far behind the camera.
        let behind = Mat4::from_translation(Vec3::new(0.0, 0.0, -1000.0));
        let path = Path::new(&out).join(format!("{name}.png"));
        r.render_frame(W, H, guest::perspective(1.0, W as f32 / H as f32, 0.1, 100.0), behind, Some(&hand), |layers| {
            let hand = layers.hand.expect("hand layer");
            write_png(&path, hand, layers.stride);
        });
        println!("wrote {}", path.display());
    }

    // The zombies HUD on its own, in a few states.
    use guest::hud::{Hud, HudState};
    use guest::zombies::Popup;
    let popups = [
        Popup { points: 10, age: 0.2, drift: 6.0 },
        Popup { points: 100, age: 0.6, drift: -10.0 },
    ];
    for (name, round, since, points, ammo, reserve) in
        [("hud_round3", 3, 10.0, 500, 30, 120), ("hud_round5", 5, 10.0, 1840, 7, 60), ("hud_round12_flash", 12, 0.3, 12650, 0, 0)]
    {
        let mut hud = Hud::new(W, H);
        hud.draw(&HudState {
            weapon: "AP9",
            ammo,
            mag: 30,
            reserve,
            reloading: false,
            aiming: false,
            bloom: 3.0,
            hitmarker: 0.1,
            crit: round == 5,
            health: if round == 12 { 30.0 } else { 100.0 },
            damage_flash: 0.0,
            prompt: if round == 3 { "Open" } else { "" },
            round,
            since_round_change: since,
            points,
            popups: &popups,
            since_max_ammo: if round == 5 { 0.5 } else { f32::MAX },
        });
        let path = Path::new(&out).join(format!("{name}.png"));
        write_png(&path, &hud.buf, hud.stride());
        println!("wrote {}", path.display());
    }
}

/// The premultiplied BGRA hand layer over a neutral grey.
fn write_png(path: &Path, bgra: &[u8], stride: usize) {
    let bg = [92.0f32, 96.0, 104.0];
    let mut rgb = Vec::with_capacity((W * H * 3) as usize);
    let mut covered = 0usize;
    for y in 0..H as usize {
        for x in 0..W as usize {
            let p = &bgra[y * stride + x * 4..y * stride + x * 4 + 4];
            let a = p[3] as f32 / 255.0;
            covered += (p[3] > 0) as usize;
            for (c, &b) in [p[2], p[1], p[0]].iter().zip(&bg) {
                rgb.push((*c as f32 + b * (1.0 - a)).min(255.0) as u8);
            }
        }
    }
    println!("  hand covers {:.1}% of the frame", covered as f32 * 100.0 / (W * H) as f32);
    // The front sight's green tritium dot, if visible: its centre in pixels.
    let (mut n, mut sx, mut sy) = (0.0f32, 0.0f32, 0.0f32);
    for y in 0..H as usize {
        for x in 0..W as usize {
            let p = &bgra[y * stride + x * 4..y * stride + x * 4 + 4];
            let (b, g, r) = (p[0] as f32, p[1] as f32, p[2] as f32);
            if g > 120.0 && g > r * 1.6 && g > b * 1.4 {
                n += 1.0;
                sx += x as f32;
                sy += y as f32;
            }
        }
    }
    if n > 0.0 {
        println!("  tritium dot at ({:.1}, {:.1}), screen centre ({}, {})", sx / n, sy / n, W / 2, H / 2);
    }
    let file = std::io::BufWriter::new(std::fs::File::create(path).expect("png"));
    let mut enc = png::Encoder::new(file, W, H);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().expect("png header").write_image_data(&rgb).expect("png data");
}
