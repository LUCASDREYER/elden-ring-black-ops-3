//! Decodes .tif files and prints their size or the error: `tifcheck <files...>`
fn main() {
    for p in std::env::args().skip(1) {
        match cod_assets::Texture::load(&p) {
            Ok(t) => println!("ok   {}x{}  {}", t.width, t.height, p.rsplit('/').next().unwrap()),
            Err(e) => println!("FAIL {e}"),
        }
    }
}
