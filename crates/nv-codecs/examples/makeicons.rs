//! Dev tool: builds the program icons from the logo (`assets/logo/nulogo.svg`, one black glyph) in one colour scheme per program.
//! Writes `assets/icons/<name>.svg` and `assets/icons/<name>.ico` (16..256 px, PNG-compressed).
//! `cargo run --release -p nv-codecs --example makeicons`
use std::path::Path;

/// (file name, light colour, dark colour): the tile is a diagonal gradient from the light to the dark one, the glyph is white.
const SCHEMES: &[(&str, &str, &str)] = &[
    ("nuviewer", "#52A8FF", "#2A49E8"),
    ("nugallery", "#2EDCC0", "#0B8F86"),
    ("nueditor", "#FFB84D", "#EE5A24"),
    ("nuquick", "#B79BFF", "#6A3FE0"),
    ("nuimporter", "#8EE06A", "#23964A"),
];

const SIZES: &[u32] = &[16, 20, 24, 32, 40, 48, 64, 96, 128, 256];

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let logo = std::fs::read_to_string(root.join("assets/logo/nulogo.svg")).expect("assets/logo/nulogo.svg");
    // The glyph group of the logo (its own transform inside), without the XML prologue.
    let g0 = logo.find("<g ").expect("<g>");
    let g1 = logo.rfind("</g>").expect("</g>") + 4;
    let glyph = &logo[g0..g1];
    std::fs::create_dir_all(root.join("assets/icons")).unwrap();
    for (name, light, dark) in SCHEMES {
        // The glyph spans about 960 x 672 of the 1080 square: scaled to 62% and centred on the tile, white, with a soft shadow below.
        let svg = format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="1080" height="1080" viewBox="0 0 1080 1080">
  <defs>
    <linearGradient id="tile" x1="0.1" y1="0" x2="0.9" y2="1"><stop offset="0" stop-color="{light}"/><stop offset="1" stop-color="{dark}"/></linearGradient>
    <linearGradient id="shine" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#ffffff" stop-opacity="0.22"/><stop offset="0.5" stop-color="#ffffff" stop-opacity="0"/></linearGradient>
  </defs>
  <rect x="40" y="40" width="1000" height="1000" rx="230" fill="url(#tile)"/>
  <rect x="40" y="40" width="1000" height="1000" rx="230" fill="url(#shine)"/>
  <g transform="translate(540 568) scale(0.64) translate(-540 -539)" fill="#000" fill-opacity="0.22" transform-origin="0 0"><g transform="translate(0 14)">{glyph}</g></g>
  <g transform="translate(540 548) scale(0.64) translate(-540 -539)" fill="#ffffff">{glyph}</g>
</svg>
"##
        );
        std::fs::write(root.join(format!("assets/icons/{name}.svg")), &svg).unwrap();
        let mut pngs: Vec<(u32, Vec<u8>)> = Vec::new();
        for &px in SIZES {
            let bgra = nv_codecs::render_icon(&svg, px, 0).expect("render");
            // premultiplied BGRA -> straight RGBA
            let mut rgba = Vec::with_capacity(bgra.len());
            for p in bgra.chunks_exact(4) {
                let a = p[3] as u32;
                let un = |c: u8| if a == 0 { 0 } else { ((c as u32 * 255 + a / 2) / a).min(255) as u8 };
                rgba.extend_from_slice(&[un(p[2]), un(p[1]), un(p[0]), p[3]]);
            }
            let mut out = Vec::new();
            {
                let mut enc = png::Encoder::new(&mut out, px, px);
                enc.set_color(png::ColorType::Rgba);
                enc.set_depth(png::BitDepth::Eight);
                enc.write_header().unwrap().write_image_data(&rgba).unwrap();
            }
            if px == 256 && *name == "nuviewer" {
                std::fs::write(root.join("assets/icons/nuviewer-256.png"), &out).unwrap();
            }
            pngs.push((px, out));
        }
        // ICO: header, directory, images.
        let mut ico = Vec::new();
        ico.extend([0u8, 0, 1, 0]);
        ico.extend((pngs.len() as u16).to_le_bytes());
        let mut offset = 6 + 16 * pngs.len();
        for (px, data) in &pngs {
            let b = if *px >= 256 { 0 } else { *px as u8 };
            ico.extend([b, b, 0, 0, 1, 0, 32, 0]);
            ico.extend((data.len() as u32).to_le_bytes());
            ico.extend((offset as u32).to_le_bytes());
            offset += data.len();
        }
        for (_, data) in &pngs {
            ico.extend(data);
        }
        std::fs::write(root.join(format!("assets/icons/{name}.ico")), &ico).unwrap();
        println!("{name}: {} bytes", ico.len());
    }
}
