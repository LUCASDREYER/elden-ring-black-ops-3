//! Prints chosen parts' poses: `poses <anim> <frame> <part>...`
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let anim = cod_assets::Anim::load(&a[1]).unwrap();
    let frame: usize = a[2].parse().unwrap();
    for name in &a[3..] {
        match anim.part_index(name) {
            Some(i) => println!("{name:<22} {:.2?}", anim.pose(frame, i).map(|p| (p.offset, p.rows))),
            None => println!("{name:<22} not animated"),
        }
    }
    let gun: Vec<_> = anim.parts.iter().filter(|p| p.to_lowercase().starts_with("tag_") || p.to_lowercase().starts_with("j_gun") || p.to_lowercase().contains("mag")).collect();
    println!("tag/gun parts: {gun:?}");
}
