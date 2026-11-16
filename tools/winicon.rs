// Embeds an .ico file as the program's icon without needing rc.exe: writes a Win32 resource file (.res: an RT_GROUP_ICON with id 1
// and one RT_ICON per picture of the .ico) and hands it to the linker. Shared by the build scripts through
// `#[path = "../tools/winicon.rs"] mod winicon;`.
use std::path::Path;

fn pad4(v: &mut Vec<u8>) {
    while v.len() % 4 != 0 {
        v.push(0);
    }
}

/// One resource entry: numeric type and name, data.
fn entry(out: &mut Vec<u8>, kind: u16, name: u16, flags: u16, data: &[u8]) {
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(32u32.to_le_bytes()); // header size: 8 + type (4) + name (4) + 16
    out.extend([0xFF, 0xFF]);
    out.extend(kind.to_le_bytes());
    out.extend([0xFF, 0xFF]);
    out.extend(name.to_le_bytes());
    out.extend(0u32.to_le_bytes()); // data version
    out.extend(flags.to_le_bytes());
    out.extend(0u16.to_le_bytes()); // language: neutral
    out.extend(0u32.to_le_bytes()); // version
    out.extend(0u32.to_le_bytes()); // characteristics
    out.extend(data);
    pad4(out);
}

pub fn embed(ico_path: &Path) {
    println!("cargo:rerun-if-changed={}", ico_path.display());
    let ico = std::fs::read(ico_path).unwrap_or_else(|e| panic!("{}: {e}", ico_path.display()));
    let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
    let mut res = Vec::new();
    // The mandatory empty first entry.
    res.extend([0u8; 4]);
    res.extend(32u32.to_le_bytes());
    res.extend([0xFF, 0xFF, 0, 0, 0xFF, 0xFF, 0, 0]);
    res.extend([0u8; 16]);
    let mut group = Vec::new();
    group.extend([0u8, 0, 1, 0]);
    group.extend((count as u16).to_le_bytes());
    for i in 0..count {
        let e = &ico[6 + 16 * i..6 + 16 * (i + 1)];
        let size = u32::from_le_bytes([e[8], e[9], e[10], e[11]]) as usize;
        let off = u32::from_le_bytes([e[12], e[13], e[14], e[15]]) as usize;
        let id = (i + 1) as u16;
        entry(&mut res, 3, id, 0x1030, &ico[off..off + size]); // RT_ICON
        // Directory entry of the group: width, height, colours, reserved, planes, bits, size, resource id.
        group.extend(&e[..8]);
        group.extend((size as u32).to_le_bytes());
        group.extend(id.to_le_bytes());
    }
    entry(&mut res, 14, 1, 0x1030, &group); // RT_GROUP_ICON, id 1: the first icon of the file
    let out = Path::new(&std::env::var("OUT_DIR").unwrap()).join("icon.res");
    std::fs::write(&out, res).unwrap();
    println!("cargo:rustc-link-arg-bins={}", out.display());
}
