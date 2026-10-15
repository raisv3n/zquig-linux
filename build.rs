use std::{env, fs, path::PathBuf};

use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::imageops::FilterType;

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.png");
    println!("cargo:rerun-if-changed=assets/zquig.manifest");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());

    // The repo keeps a single PNG icon; build the multi-size .ico Windows wants from it.
    let icon = image::open("assets/icon.png").expect("assets/icon.png").to_rgba8();
    let frames: Vec<IcoFrame> = [256u32, 64, 48, 32, 24, 16]
        .iter()
        .map(|&s| {
            let img = image::imageops::resize(&icon, s, s, FilterType::Lanczos3);
            IcoFrame::as_png(img.as_raw(), s, s, image::ExtendedColorType::Rgba8).unwrap()
        })
        .collect();
    let ico = out.join("zquig.ico");
    IcoEncoder::new(fs::File::create(&ico).unwrap()).encode_images(&frames).unwrap();

    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("assets/zquig.manifest");
    let rc = out.join("zquig.rc");
    let esc = |p: &PathBuf| p.display().to_string().replace('\\', "\\\\");
    fs::write(&rc, format!("1 ICON \"{}\"\n1 24 \"{}\"\n", esc(&ico), esc(&manifest))).unwrap();
    embed_resource::compile(&rc, embed_resource::NONE).manifest_required().unwrap();
}
