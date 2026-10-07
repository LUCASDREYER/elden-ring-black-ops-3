//! The HUD, drawn on the CPU into the GUI layer (BGRA, premultiplied, rows top-down), in the
//! style of Black Ops 3 Zombies: the red chalk round counter (tallies up to five, then
//! numerals), the score with "+10" pop-ups, the ammo as magazine and reserve, the crosshair and
//! hit marker, the red damage edges, Elden Ring's action prompt and the Max Ammo banner.
//!
//! The artwork is drawn here, not taken from the game: the mod tools don't include BO3's
//! zombies HUD images.

use crate::zombies::Popup;

pub struct HudState<'a> {
    pub weapon: &'a str,
    pub ammo: u32,
    pub mag: u32,
    pub reserve: u32,
    pub reloading: bool,
    /// Aiming down the sights: no crosshair, as in Call of Duty.
    pub aiming: bool,
    /// Crosshair gap in pixels beyond the base gap (spread after firing).
    pub bloom: f32,
    /// 0..1, fading hit marker; red when `crit`.
    pub hitmarker: f32,
    pub crit: bool,
    /// 0..100
    pub health: f32,
    /// 0..1 after taking a hit.
    pub damage_flash: f32,
    /// Elden Ring's action on offer, if any ("Open", "Pick up").
    pub prompt: &'a str,
    pub round: u32,
    /// Seconds since the round changed.
    pub since_round_change: f32,
    pub points: u32,
    pub popups: &'a [Popup],
    /// Seconds since a Max Ammo dropped.
    pub since_max_ammo: f32,
}

pub struct Hud {
    pub width: u32,
    pub height: u32,
    pub buf: Vec<u8>,
}

#[derive(Clone, Copy)]
struct Rgba(f32, f32, f32, f32);

const WHITE: Rgba = Rgba(1.0, 1.0, 1.0, 0.92);
const SHADOW: Rgba = Rgba(0.0, 0.0, 0.0, 0.55);
const CHALK_RED: Rgba = Rgba(0.62, 0.04, 0.02, 0.95);
const GOLD: Rgba = Rgba(1.0, 0.86, 0.35, 0.95);

/// Cheap per-pixel noise in 0..1, for the chalk texture.
fn noise(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h & 0xFFFF) as f32 / 65535.0
}

fn mix(a: Rgba, b: Rgba, t: f32) -> Rgba {
    Rgba(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t, a.3 + (b.3 - a.3) * t)
}

impl Hud {
    pub fn new(width: u32, height: u32) -> Hud {
        Hud { width, height, buf: vec![0; (width * height * 4) as usize] }
    }

    pub fn stride(&self) -> usize {
        self.width as usize * 4
    }

    fn blend(&mut self, x: i32, y: i32, c: Rgba) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 || c.3 <= 0.0 {
            return;
        }
        let i = ((y as u32 * self.width + x as u32) * 4) as usize;
        let p = &mut self.buf[i..i + 4];
        // Premultiplied "over", stored BGRA.
        let src = [c.2 * c.3, c.1 * c.3, c.0 * c.3, c.3];
        for k in 0..4 {
            let dst = p[k] as f32 / 255.0;
            p[k] = ((src[k] + dst * (1.0 - c.3)).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        }
    }

    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgba) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.blend(xx, yy, c);
            }
        }
    }

    /// A line `t` pixels thick, drawn as a run of squares.
    fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, t: i32, c: Rgba) {
        let n = ((x1 - x0).abs().max((y1 - y0).abs()).ceil() as i32).max(1);
        for i in 0..=n {
            let f = i as f32 / n as f32;
            let (x, y) = (x0 + (x1 - x0) * f, y0 + (y1 - y0) * f);
            self.rect(x as i32 - t / 2, y as i32 - t / 2, t, t, c);
        }
    }

    /// A hand-drawn chalk stroke: wobbly, `width` wide, grainy.
    fn chalk_stroke(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, c: Rgba, seed: i32) {
        let len = ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt().max(1.0);
        let (dx, dy) = ((x1 - x0) / len, (y1 - y0) / len);
        let (nx, ny) = (-dy, dx);
        let steps = (len * 1.5) as i32;
        for i in 0..=steps {
            let f = i as f32 / steps as f32;
            let wobble = (f * 9.0 + seed as f32 * 1.7).sin() * width * 0.12;
            // Thicker in the middle of the stroke, thinner at its ends.
            let w = width * (0.75 + 0.35 * (f * std::f32::consts::PI).sin());
            let (cx, cy) = (x0 + (x1 - x0) * f + nx * wobble, y0 + (y1 - y0) * f + ny * wobble);
            let half = (w * 0.5) as i32;
            for k in -half..=half {
                let (px, py) = ((cx + nx * k as f32) as i32, (cy + ny * k as f32) as i32);
                let g = noise(px + seed * 131, py);
                if g < 0.16 {
                    continue; // gaps where the chalk skipped
                }
                let edge = 1.0 - (k.abs() as f32 / (half as f32 + 1.0)).powi(3);
                self.blend(px, py, Rgba(c.0, c.1, c.2, c.3 * edge * (0.55 + 0.45 * g)));
            }
        }
    }

    /// Big hand-drawn numerals: each digit a few chalk strokes, `height` pixels tall.
    fn chalk_digits(&mut self, s: &str, x: f32, y: f32, height: f32, c: Rgba) {
        let width = height * 0.62;
        let stroke = height * 0.13;
        let mut cx = x;
        for (n, ch) in s.chars().enumerate() {
            for (k, path) in digit_strokes(ch).iter().enumerate() {
                for (i, seg) in path.windows(2).enumerate() {
                    let (a, b) = (seg[0], seg[1]);
                    self.chalk_stroke(
                        cx + a.0 * width,
                        y + a.1 * height,
                        cx + b.0 * width,
                        y + b.1 * height,
                        stroke,
                        c,
                        (n * 31 + k * 7 + i) as i32,
                    );
                }
            }
            cx += width + height * 0.18;
        }
    }

    fn text(&mut self, s: &str, x: i32, y: i32, scale: i32, c: Rgba) {
        let mut cx = x;
        for ch in s.chars() {
            let g = glyph(ch.to_ascii_uppercase());
            for (row, bits) in g.iter().enumerate() {
                for col in 0..5 {
                    if bits & (0x10 >> col) != 0 {
                        let (px, py) = (cx + col * scale, y + row as i32 * scale);
                        self.rect(px + 1, py + 1, scale, scale, SHADOW);
                        self.rect(px, py, scale, scale, c);
                    }
                }
            }
            cx += 6 * scale;
        }
    }

    fn text_width(s: &str, scale: i32) -> i32 {
        s.chars().count() as i32 * 6 * scale - scale
    }

    /// The round counter: tallies for rounds 1 to 5 (the fifth strikes through the four),
    /// numerals after that. Flashes to white when the round changes.
    fn round_counter(&mut self, round: u32, since_change: f32, ui: f32) {
        let (h, base_x) = (self.height as f32, 42.0 * ui);
        let top = h - 150.0 * ui;
        let flash = if since_change < 4.0 { (1.0 - since_change / 4.0) * (0.5 + 0.5 * (since_change * 7.0).cos()) } else { 0.0 };
        let c = mix(CHALK_RED, Rgba(1.0, 1.0, 1.0, 0.95), flash);
        if (1..=5).contains(&round) {
            let height = 92.0 * ui;
            let gap = 21.0 * ui;
            for i in 0..round.min(4) as i32 {
                let x = base_x + i as f32 * gap;
                let lean = if i % 2 == 0 { 4.0 } else { -3.0 } * ui;
                self.chalk_stroke(x, top, x + lean, top + height, 11.0 * ui, c, i + 1);
            }
            if round == 5 {
                self.chalk_stroke(base_x - 12.0 * ui, top + height * 0.72, base_x + gap * 3.0 + 14.0 * ui, top + height * 0.22, 10.0 * ui, c, 9);
            }
        } else {
            self.chalk_digits(&round.to_string(), base_x, top, 92.0 * ui, c);
        }
    }

    pub fn draw(&mut self, s: &HudState) {
        self.buf.fill(0);
        let (w, h) = (self.width as i32, self.height as i32);
        let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
        let ui = (h as f32 / 720.0).max(0.5);

        // Hurt: red at the edges, stronger the lower the health.
        let hurt = (s.damage_flash + (1.0 - s.health / 100.0).max(0.0) * 0.7).min(1.0);
        if hurt > 0.0 {
            let band = (70.0 * ui) as i32;
            for i in 0..band {
                let a = 0.5 * hurt * (1.0 - i as f32 / band as f32).powi(2);
                let c = Rgba(0.7, 0.03, 0.02, a);
                self.rect(i, i, w - 2 * i, 1, c);
                self.rect(i, h - 1 - i, w - 2 * i, 1, c);
                self.rect(i, i + 1, 1, h - 2 * i - 2, c);
                self.rect(w - 1 - i, i + 1, 1, h - 2 * i - 2, c);
            }
        }

        // Crosshair: four ticks around the centre, pushed out by bloom.
        if !s.reloading && !s.aiming {
            let gap = (7.0 + s.bloom) * ui;
            let len = 9.0 * ui;
            let t = (2.0 * ui).round().max(1.0) as i32;
            for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                let (x0, y0) = (cx + dx * gap, cy + dy * gap);
                let (x1, y1) = (cx + dx * (gap + len), cy + dy * (gap + len));
                self.line(x0 + 1.0, y0 + 1.0, x1 + 1.0, y1 + 1.0, t, SHADOW);
                self.line(x0, y0, x1, y1, t, WHITE);
            }
        }
        if s.hitmarker > 0.0 {
            let a = (s.hitmarker / 0.15).clamp(0.0, 1.0);
            let c = if s.crit { Rgba(1.0, 0.25, 0.2, a) } else { Rgba(1.0, 1.0, 1.0, a) };
            let (r0, r1) = (6.0 * ui, 13.0 * ui);
            let t = (2.0 * ui).round().max(1.0) as i32;
            for (dx, dy) in [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
                self.line(cx + dx * r0, cy + dy * r0, cx + dx * r1, cy + dy * r1, t, c);
            }
        }

        self.round_counter(s.round, s.since_round_change, ui);

        // Bottom right: weapon, magazine | reserve, and the score above them.
        let right = w - (42.0 * ui) as i32;
        let big = (4.0 * ui).round().max(2.0) as i32;
        let small = (2.0 * ui).round().max(1.0) as i32;
        let ammo_y = h - (54.0 * ui) as i32;
        let low = s.ammo * 4 <= s.mag;
        let reserve = format!("| {}", s.reserve);
        let reserve_x = right - Self::text_width(&reserve, small);
        self.text(&reserve, reserve_x, ammo_y + 7 * big - 7 * small, small, if s.reserve == 0 { Rgba(1.0, 0.35, 0.25, 0.95) } else { WHITE });
        let ammo = s.ammo.to_string();
        let ammo_c = if low { Rgba(1.0, 0.35, 0.25, 0.95) } else { WHITE };
        self.text(&ammo, reserve_x - Self::text_width(&ammo, big) - 3 * small, ammo_y, big, ammo_c);
        self.text(s.weapon, right - Self::text_width(s.weapon, small), ammo_y - 12 * small, small, Rgba(1.0, 1.0, 1.0, 0.75));

        let score = s.points.to_string();
        let score_scale = (3.0 * ui).round().max(2.0) as i32;
        let score_y = ammo_y - (70.0 * ui) as i32;
        let score_x = right - Self::text_width(&score, score_scale);
        self.rect(score_x - 10, score_y - 8, right - score_x + 20, 7 * score_scale + 16, Rgba(0.0, 0.0, 0.0, 0.35));
        self.text(&score, score_x, score_y, score_scale, WHITE);
        for p in s.popups {
            let t = p.age / 1.2;
            let a = (1.0 - t).clamp(0.0, 1.0);
            let label = format!("+{}", p.points);
            let x = score_x - Self::text_width(&label, small) - (18.0 * ui) as i32 + (p.drift * t * ui) as i32;
            let y = score_y - (t * 40.0 * ui) as i32;
            self.text(&label, x, y, small, Rgba(GOLD.0, GOLD.1, GOLD.2, a));
        }

        if s.reloading {
            let msg = "RELOADING";
            self.text(msg, (cx as i32) - Self::text_width(msg, small) / 2, (cy + 40.0 * ui) as i32, small, WHITE);
        } else if s.ammo == 0 && s.reserve == 0 {
            let msg = "NO AMMO";
            self.text(msg, (cx as i32) - Self::text_width(msg, small) / 2, (cy + 40.0 * ui) as i32, small, Rgba(1.0, 0.35, 0.25, 0.95));
        } else if low {
            let msg = if s.ammo == 0 { "RELOAD" } else { "LOW AMMO" };
            self.text(msg, (cx as i32) - Self::text_width(msg, small) / 2, (cy + 40.0 * ui) as i32, small, ammo_c);
        }

        if s.since_max_ammo < 2.5 {
            let a = (1.0 - (s.since_max_ammo - 1.8).max(0.0) / 0.7).clamp(0.0, 1.0);
            let msg = "MAX AMMO!";
            let sc = (4.0 * ui).round() as i32;
            self.text(msg, (cx as i32) - Self::text_width(msg, sc) / 2, (90.0 * ui) as i32, sc, Rgba(GOLD.0, GOLD.1, GOLD.2, a));
        }

        if !s.prompt.is_empty() {
            let msg = format!("F  {}", s.prompt);
            self.text(&msg, (cx as i32) - Self::text_width(&msg, small) / 2, (cy + 90.0 * ui) as i32, small, WHITE);
        }
    }
}

/// Chalk strokes for each digit, in a unit box (x right, y down): polylines.
fn digit_strokes(c: char) -> Vec<Vec<(f32, f32)>> {
    let oval = |cx: f32, cy: f32, rx: f32, ry: f32| -> Vec<(f32, f32)> {
        (0..=14).map(|k| {
            let a = k as f32 / 14.0 * std::f32::consts::TAU;
            (cx + rx * a.sin(), cy - ry * a.cos())
        })
        .collect()
    };
    match c {
        '0' => vec![oval(0.5, 0.5, 0.42, 0.5)],
        '1' => vec![vec![(0.28, 0.18), (0.55, 0.0), (0.55, 1.0)], vec![(0.25, 1.0), (0.85, 1.0)]],
        '2' => vec![vec![(0.1, 0.25), (0.3, 0.03), (0.7, 0.03), (0.9, 0.25), (0.82, 0.48), (0.1, 1.0), (0.95, 1.0)]],
        '3' => vec![vec![(0.1, 0.1), (0.5, 0.0), (0.88, 0.15), (0.82, 0.4), (0.42, 0.5), (0.88, 0.6), (0.92, 0.85), (0.5, 1.0), (0.08, 0.9)]],
        '4' => vec![vec![(0.72, 1.0), (0.72, 0.0), (0.05, 0.7), (0.98, 0.7)]],
        '5' => vec![vec![(0.9, 0.0), (0.2, 0.0), (0.14, 0.45), (0.6, 0.38), (0.92, 0.6), (0.88, 0.88), (0.5, 1.0), (0.08, 0.9)]],
        '6' => vec![vec![(0.85, 0.05), (0.4, 0.1), (0.12, 0.5), (0.18, 0.9), (0.5, 1.0), (0.85, 0.85), (0.85, 0.6), (0.5, 0.48), (0.14, 0.62)]],
        '7' => vec![vec![(0.05, 0.0), (0.95, 0.0), (0.4, 1.0)]],
        '8' => vec![oval(0.5, 0.24, 0.32, 0.24), oval(0.5, 0.73, 0.4, 0.27)],
        '9' => vec![vec![(0.86, 0.4), (0.5, 0.52), (0.15, 0.38), (0.18, 0.1), (0.5, 0.0), (0.85, 0.12), (0.86, 0.45), (0.76, 0.8), (0.4, 1.0), (0.15, 0.95)]],
        _ => Vec::new(),
    }
}

/// 5x7 glyphs, rows top to bottom, bit 4 is the left column.
fn glyph(c: char) -> [u8; 7] {
    match c {
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'D' => [0x1C, 0x12, 0x11, 0x11, 0x11, 0x12, 0x1C],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x11, 0x19, 0x15, 0x13, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
        'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x11, 0x0A, 0x04, 0x04, 0x04],
        'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        '/' => [0x01, 0x01, 0x02, 0x04, 0x08, 0x10, 0x10],
        '|' => [0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        '+' => [0x00, 0x04, 0x04, 0x1F, 0x04, 0x04, 0x00],
        '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C],
        ':' => [0x00, 0x0C, 0x0C, 0x00, 0x0C, 0x0C, 0x00],
        '!' => [0x04, 0x04, 0x04, 0x04, 0x04, 0x00, 0x04],
        '\'' => [0x04, 0x04, 0x08, 0x00, 0x00, 0x00, 0x00],
        _ => [0; 7],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn sample(round: u32) -> HudState<'static> {
        HudState {
            weapon: "AP9",
            ammo: 30,
            mag: 30,
            reserve: 120,
            reloading: false,
            aiming: false,
            bloom: 0.0,
            hitmarker: 0.0,
            crit: false,
            health: 100.0,
            damage_flash: 0.0,
            prompt: "",
            round,
            since_round_change: 10.0,
            points: 500,
            popups: &[],
            since_max_ammo: f32::MAX,
        }
    }

    #[test]
    fn draws_crosshair_and_round_and_keeps_the_middle_clear() {
        let mut hud = Hud::new(1280, 720);
        hud.draw(&sample(3));
        let alpha = |x: u32, y: u32| hud.buf[((y * 1280 + x) * 4 + 3) as usize];
        assert_eq!(alpha(640, 360), 0, "the very centre stays open");
        assert!((0..1280).any(|x| alpha(x, 360) > 0), "crosshair ticks drawn");
        assert!((570..680).any(|y| (40..120).any(|x| alpha(x, y) > 0)), "round tallies drawn bottom left");
        assert_eq!(alpha(5, 5), 0, "corners clear");
    }
}
