//! Dev tool: builds the program icons from the logo (`assets/logo/nulogo.svg`, one black glyph).
//! Writes `assets/icons/<name>.svg` (the full icon), `assets/icons/<name>.ico` (16..256 px, PNG-compressed) and, with
//! `MAKEICONS_PREVIEW=<dir>`, one PNG per size and program for looking at.
//! `cargo run --release -p nv-codecs --example makeicons`
//!
//! The look (version 0.6): a dark, glassy squircle with a thin light edge and a soft glow in the programs' colour, the "nu"
//! mark in a gradient of that colour (no white-on-colour tile, no hard shadow), and a small round badge with what the program
//! does (a picture, a grid, sliders, a tray, a bolt). The sizes up to 24 px leave the badge out and draw the mark larger, because
//! a badge only turns into a smudge there.
use std::path::Path;

struct Scheme {
    name: &'static str,
    /// Light and dark end of the colour (the mark's gradient and the badge), and the glow behind the mark.
    light: &'static str,
    dark: &'static str,
    /// The badge pictogram in a 100 x 100 box: SVG elements drawn in white (stroke / fill set by the element itself).
    badge: &'static str,
}

const SCHEMES: &[Scheme] = &[
    // A picture: frame, sun and hills.
    Scheme {
        name: "nuviewer",
        light: "#6CC4FF",
        dark: "#3B6BFF",
        badge: r##"<rect x="18" y="22" width="64" height="56" rx="10" fill="none" stroke="#fff" stroke-width="8"/><circle cx="38" cy="42" r="7" fill="#fff"/><path d="M22 72 L45 50 L60 63 L70 54 L80 64 L80 72 Z" fill="#fff"/>"##,
    },
    // A grid of tiles.
    Scheme {
        name: "nugallery",
        light: "#5CF0D2",
        dark: "#0FA3A0",
        badge: r##"<rect x="20" y="20" width="26" height="26" rx="7" fill="#fff"/><rect x="54" y="20" width="26" height="26" rx="7" fill="#fff"/><rect x="20" y="54" width="26" height="26" rx="7" fill="#fff"/><rect x="54" y="54" width="26" height="26" rx="7" fill="#fff" fill-opacity="0.65"/>"##,
    },
    // Three sliders.
    Scheme {
        name: "nueditor",
        light: "#FFC857",
        dark: "#F2542D",
        badge: r##"<path d="M22 30 H78 M22 50 H78 M22 70 H78" stroke="#fff" stroke-width="7" stroke-linecap="round" stroke-opacity="0.55"/><circle cx="38" cy="30" r="9" fill="#fff"/><circle cx="62" cy="50" r="9" fill="#fff"/><circle cx="45" cy="70" r="9" fill="#fff"/>"##,
    },
    // A bolt: it is there at once.
    Scheme {
        name: "nuquick",
        light: "#C3A6FF",
        dark: "#7C4DFF",
        badge: r##"<path d="M56 14 L28 55 H48 L42 86 L72 44 H52 Z" fill="#fff" stroke="#fff" stroke-width="3" stroke-linejoin="round"/>"##,
    },
    // A page of text with lines.
    Scheme {
        name: "nunote",
        light: "#FF9EC4",
        dark: "#E0407F",
        badge: r##"<path d="M30 14 H58 L74 30 V84 Q74 88 70 88 H30 Q26 88 26 84 V18 Q26 14 30 14 Z" fill="none" stroke="#fff" stroke-width="7" stroke-linejoin="round"/><path d="M58 14 V30 H74" fill="none" stroke="#fff" stroke-width="6" stroke-linejoin="round"/><path d="M36 48 H64 M36 60 H64 M36 72 H54" stroke="#fff" stroke-width="6" stroke-linecap="round"/>"##,
    },
    // An arrow into a tray.
    Scheme {
        name: "nuimporter",
        light: "#9BEA6E",
        dark: "#25A55A",
        badge: r##"<path d="M50 16 V56 M32 40 L50 58 L68 40" fill="none" stroke="#fff" stroke-width="9" stroke-linecap="round" stroke-linejoin="round"/><path d="M20 66 V76 Q20 82 26 82 H74 Q80 82 80 76 V66" fill="none" stroke="#fff" stroke-width="8" stroke-linecap="round"/>"##,
    },
];

const SIZES: &[u32] = &[16, 20, 24, 32, 40, 48, 64, 96, 128, 256];
/// Up to this size the badge is left out.
const SMALL: u32 = 24;

/// The icon as SVG. `small`: no badge, bigger mark.
fn icon_svg(s: &Scheme, glyph: &str, small: bool) -> String {
    let (light, dark) = (s.light, s.dark);
    // The mark spans about 960 x 672 of the 1080 square. Full icon: 52% wide, a little above the middle; small: 70%.
    let (scale, cx, cy) = if small { (0.70, 540.0, 548.0) } else { (0.54, 520.0, 470.0) };
    let mark = format!(r##"<g transform="translate({cx} {cy}) scale({scale}) translate(-540 -539)">{glyph}</g>"##);
    let badge = if small {
        String::new()
    } else {
        format!(
            r##"<g transform="translate(790 790)">
    <circle r="178" fill="#0F1218"/>
    <circle r="150" fill="url(#mark)"/>
    <circle r="150" fill="url(#bshine)"/>
    <g transform="translate(-90 -90) scale(1.8)">{}</g>
  </g>"##,
            s.badge
        )
    };
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="1080" height="1080" viewBox="0 0 1080 1080">
  <defs>
    <linearGradient id="tile" x1="0.15" y1="0" x2="0.85" y2="1"><stop offset="0" stop-color="#2A3042"/><stop offset="0.55" stop-color="#171B27"/><stop offset="1" stop-color="#0D0F15"/></linearGradient>
    <radialGradient id="glow" cx="0.28" cy="0.2" r="0.85"><stop offset="0" stop-color="{light}" stop-opacity="0.38"/><stop offset="0.55" stop-color="{dark}" stop-opacity="0.10"/><stop offset="1" stop-color="{dark}" stop-opacity="0"/></radialGradient>
    <linearGradient id="mark" x1="0.1" y1="0" x2="0.9" y2="1"><stop offset="0" stop-color="{light}"/><stop offset="1" stop-color="{dark}"/></linearGradient>
    <linearGradient id="edge" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#ffffff" stop-opacity="0.34"/><stop offset="0.45" stop-color="#ffffff" stop-opacity="0.06"/><stop offset="1" stop-color="#ffffff" stop-opacity="0.16"/></linearGradient>
    <linearGradient id="bshine" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#ffffff" stop-opacity="0.28"/><stop offset="0.6" stop-color="#ffffff" stop-opacity="0"/></linearGradient>
    <filter id="blur" x="-30%" y="-30%" width="160%" height="160%"><feGaussianBlur stdDeviation="34"/></filter>
  </defs>
  <rect x="40" y="40" width="1000" height="1000" rx="236" fill="url(#tile)"/>
  <rect x="40" y="40" width="1000" height="1000" rx="236" fill="url(#glow)"/>
  <rect x="43" y="43" width="994" height="994" rx="233" fill="none" stroke="url(#edge)" stroke-width="6"/>
  <g filter="url(#blur)" opacity="0.55" fill="{dark}">{mark}</g>
  <g fill="url(#mark)">{mark}</g>
  {badge}
</svg>
"##
    )
}

fn rgba_of(bgra: &[u8]) -> Vec<u8> {
    // premultiplied BGRA -> straight RGBA
    let mut rgba = Vec::with_capacity(bgra.len());
    for p in bgra.chunks_exact(4) {
        let a = p[3] as u32;
        let un = |c: u8| if a == 0 { 0 } else { ((c as u32 * 255 + a / 2) / a).min(255) as u8 };
        rgba.extend_from_slice(&[un(p[2]), un(p[1]), un(p[0]), p[3]]);
    }
    rgba
}

fn png_of(rgba: &[u8], px: u32) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, px, px);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(rgba).unwrap();
    }
    out
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let logo = std::fs::read_to_string(root.join("assets/logo/nulogo.svg")).expect("assets/logo/nulogo.svg");
    // The glyph group of the logo (its own transform inside), without the XML prologue.
    let g0 = logo.find("<g ").expect("<g>");
    let g1 = logo.rfind("</g>").expect("</g>") + 4;
    let glyph = &logo[g0..g1];
    std::fs::create_dir_all(root.join("assets/icons")).unwrap();
    let preview = std::env::var_os("MAKEICONS_PREVIEW").map(std::path::PathBuf::from);
    if let Some(d) = &preview {
        std::fs::create_dir_all(d).unwrap();
    }
    for s in SCHEMES {
        let (full, small) = (icon_svg(s, glyph, false), icon_svg(s, glyph, true));
        std::fs::write(root.join(format!("assets/icons/{}.svg", s.name)), &full).unwrap();
        let mut pngs: Vec<(u32, Vec<u8>)> = Vec::new();
        for &px in SIZES {
            let svg = if px <= SMALL { &small } else { &full };
            let bgra = nv_codecs::render_icon(svg, px, 0).expect("render");
            let png = png_of(&rgba_of(&bgra), px);
            if px == 256 && s.name == "nuviewer" {
                std::fs::write(root.join("assets/icons/nuviewer-256.png"), &png).unwrap();
            }
            if let Some(d) = &preview {
                std::fs::write(d.join(format!("{}-{px}.png", s.name)), &png).unwrap();
            }
            pngs.push((px, png));
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
        std::fs::write(root.join(format!("assets/icons/{}.ico", s.name)), &ico).unwrap();
        println!("{}: {} bytes", s.name, ico.len());
    }
}
