//! Prints what's inside x*_bin files: `cargo run -p cod-assets --example inspect -- <files...>`
use cod_assets::{Anim, Model};

fn main() {
    for path in std::env::args().skip(1) {
        let lower = path.to_lowercase();
        if lower.ends_with("xmodel_bin") {
            match Model::load(&path) {
                Ok(m) => {
                    let mut lo = [f32::MAX; 3];
                    let mut hi = [f32::MIN; 3];
                    for v in &m.verts {
                        for k in 0..3 {
                            lo[k] = lo[k].min(v.pos[k]);
                            hi[k] = hi[k].max(v.pos[k]);
                        }
                    }
                    println!("MODEL {path}\n  version {} bones {} verts {} faces {} objects {} materials {}",
                        m.version, m.bones.len(), m.verts.len(), m.faces.len(), m.objects.len(), m.materials.len());
                    println!("  bounds {lo:.2?} .. {hi:.2?}");
                    for (i, b) in m.bones.iter().enumerate().take(12) {
                        println!("  bone {i:3} {:<28} parent {:3} at {:.2?} x{:.2?}", b.name, b.parent, b.bind.offset, b.bind.rows[0]);
                    }
                    if m.bones.len() > 12 {
                        let rest: Vec<_> = m.bones[12..].iter().map(|b| b.name.as_str()).collect();
                        println!("  ... {}", rest.join(" "));
                    }
                    for mat in &m.materials {
                        println!("  material {} ({}) {:?}", mat.name, mat.kind, mat.images);
                    }
                    println!("  objects {:?}", m.objects);
                    let maxw = m.verts.iter().map(|v| v.weights.len()).max().unwrap_or(0);
                    println!("  max weights per vertex {maxw}");
                }
                Err(e) => println!("MODEL {path}: ERROR {e}"),
            }
        } else if lower.ends_with("xanim_bin") {
            match Anim::load(&path) {
                Ok(a) => {
                    println!("ANIM {path}\n  version {} parts {} frames {} @ {} fps, notes {:?}",
                        a.version, a.parts.len(), a.frames.len(), a.framerate,
                        a.notes.iter().map(|n| format!("{}@{}", n.name, n.frame)).collect::<Vec<_>>());
                    println!("  parts {}", a.parts.join(" "));
                    if let Some(i) = a.part_index("tag_weapon").or(a.part_index("j_gun")) {
                        println!("  {} frame 0 {:.2?}", a.parts[i], a.pose(0, i));
                    }
                }
                Err(e) => println!("ANIM {path}: ERROR {e}"),
            }
        }
    }
}
