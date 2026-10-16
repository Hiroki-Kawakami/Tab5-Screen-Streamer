//! Embeds icons/app.ico as the Windows executable's icon, as a compiled
//! resource file the MSVC linker takes directly.

use std::env;
use std::fs;
use std::path::PathBuf;

const ICON: &str = "icons/app.ico";
const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const GROUP_ID: u16 = 1;
/// MOVEABLE | PURE | DISCARDABLE, what rc.exe writes; Windows ignores them.
const MEMORY_FLAGS: u16 = 0x1030;

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

/// One entry of a .res file, with a numeric type and name.
fn resource(out: &mut Vec<u8>, kind: u16, id: u16, data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&32u32.to_le_bytes());
    for value in [0xffff, kind, 0xffff, id] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    let flags = if kind == 0 { 0 } else { MEMORY_FLAGS };
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(data);
    out.resize(out.len().next_multiple_of(4), 0);
}

fn main() {
    println!("cargo:rerun-if-changed={ICON}");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        println!("cargo:warning=the icon is embedded only with the MSVC linker");
        return;
    }
    let ico = fs::read(ICON).expect(ICON);
    let count = u16_at(&ico, 4);
    let mut res = Vec::new();
    resource(&mut res, 0, 0, &[]);
    let mut group = Vec::new();
    for value in [0, 1, count] {
        group.extend_from_slice(&value.to_le_bytes());
    }
    for index in 0..count {
        let entry = &ico[6 + index as usize * 16..][..16];
        let bytes = u32_at(entry, 8) as usize;
        let offset = u32_at(entry, 12) as usize;
        let id = index + 1;
        resource(&mut res, RT_ICON, id, &ico[offset..offset + bytes]);
        group.extend_from_slice(&entry[..12]);
        group.extend_from_slice(&id.to_le_bytes());
    }
    resource(&mut res, RT_GROUP_ICON, GROUP_ID, &group);
    let path = PathBuf::from(env::var("OUT_DIR").unwrap()).join("app.res");
    fs::write(&path, res).expect("app.res");
    println!("cargo:rustc-link-arg-bins={}", path.display());
}
