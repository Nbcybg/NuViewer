//! Per-user file association registration (no admin rights): makes NuViewer show up under "Open with" and in
//! Settings > Apps > Default apps. Windows 10/11 deliberately won't let a program make *itself* the default;
//! the user picks NuViewer there (or "Open with > Always"). Everything lives under HKEY_CURRENT_USER.

use std::path::Path;
use windows::core::PCWSTR;
use windows::Win32::System::Registry::*;

/// The ProgID our file types point at.
pub const PROG_ID: &str = "NuViewer.Image";
const APP_NAME: &str = "NuViewer";
const APP_DESCRIPTION: &str = "Fast, lightweight image viewer";

/// Registry locations (all relative to HKEY_CURRENT_USER). A separate layout lets tests run in a scratch key.
pub struct Layout {
    pub classes: String,
    pub app_key: String,
    pub registered_apps: String,
}

impl Layout {
    pub fn real() -> Self {
        Layout { classes: "Software\\Classes".into(), app_key: "Software\\NuViewer".into(), registered_apps: "Software\\RegisteredApplications".into() }
    }

    pub fn sandbox(prefix: &str) -> Self {
        Layout { classes: format!("{prefix}\\Classes"), app_key: format!("{prefix}\\NuViewer"), registered_apps: format!("{prefix}\\RegisteredApplications") }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn check(e: windows::Win32::Foundation::WIN32_ERROR, what: &str) -> Result<(), String> {
    e.ok().map_err(|err| format!("{what}: {err}"))
}

/// Create (or open) `subkey` under HKCU for writing.
fn open_write(subkey: &str) -> Result<HKEY, String> {
    let sub = wide(subkey);
    let mut key = HKEY::default();
    check(
        unsafe { RegCreateKeyExW(HKEY_CURRENT_USER, PCWSTR(sub.as_ptr()), None, PCWSTR::null(), REG_OPTION_NON_VOLATILE, KEY_WRITE, None, &mut key, None) },
        &format!("create {subkey}"),
    )?;
    Ok(key)
}

/// Set a string value (`name` = None sets the key's default value).
pub fn set_string(subkey: &str, name: Option<&str>, value: &str) -> Result<(), String> {
    let key = open_write(subkey)?;
    let n = name.map(wide);
    let data: Vec<u8> = wide(value).iter().flat_map(|c| c.to_le_bytes()).collect();
    let r = unsafe { RegSetValueExW(key, n.as_ref().map(|n| PCWSTR(n.as_ptr())).unwrap_or(PCWSTR::null()), None, REG_SZ, Some(&data)) };
    unsafe {
        let _ = RegCloseKey(key);
    }
    check(r, &format!("set {subkey}"))
}

/// Set a REG_DWORD value.
pub fn set_dword(subkey: &str, name: &str, value: u32) -> Result<(), String> {
    let key = open_write(subkey)?;
    let n = wide(name);
    let r = unsafe { RegSetValueExW(key, PCWSTR(n.as_ptr()), None, REG_DWORD, Some(&value.to_le_bytes())) };
    unsafe {
        let _ = RegCloseKey(key);
    }
    check(r, &format!("set {subkey}"))
}

/// Set an empty REG_NONE value (how `OpenWithProgids` entries are stored).
fn set_marker(subkey: &str, name: &str) -> Result<(), String> {
    let key = open_write(subkey)?;
    let n = wide(name);
    let r = unsafe { RegSetValueExW(key, PCWSTR(n.as_ptr()), None, REG_NONE, Some(&[])) };
    unsafe {
        let _ = RegCloseKey(key);
    }
    check(r, &format!("set {subkey}"))
}

pub fn delete_tree(subkey: &str) {
    let sub = wide(subkey);
    unsafe {
        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(sub.as_ptr()));
    }
}

pub fn delete_value(subkey: &str, name: &str) {
    let (sub, n) = (wide(subkey), wide(name));
    unsafe {
        let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, PCWSTR(sub.as_ptr()), PCWSTR(n.as_ptr()));
    }
}

/// Read a string value back (None if the key or value is missing). Used by tests and `is_registered`.
pub fn read_string(subkey: &str, name: Option<&str>) -> Option<String> {
    let (sub, n) = (wide(subkey), name.map(wide));
    let mut size = 0u32;
    let name_ptr = n.as_ref().map(|n| PCWSTR(n.as_ptr())).unwrap_or(PCWSTR::null());
    unsafe {
        if RegGetValueW(HKEY_CURRENT_USER, PCWSTR(sub.as_ptr()), name_ptr, RRF_RT_REG_SZ, None, None, Some(&mut size)).is_err() {
            return None;
        }
        let mut buf = vec![0u16; size as usize / 2 + 1];
        let mut size2 = (buf.len() * 2) as u32;
        if RegGetValueW(HKEY_CURRENT_USER, PCWSTR(sub.as_ptr()), name_ptr, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut size2)).is_err() {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }
}

/// True if a value (of any type) exists.
#[cfg(test)]
fn value_exists(subkey: &str, name: &str) -> bool {
    let (sub, n) = (wide(subkey), wide(name));
    unsafe { RegGetValueW(HKEY_CURRENT_USER, PCWSTR(sub.as_ptr()), PCWSTR(n.as_ptr()), RRF_RT_ANY, None, None, None).is_ok() }
}

pub fn is_registered(layout: &Layout) -> bool {
    read_string(&format!("{}\\{PROG_ID}\\shell\\open\\command", layout.classes), None).is_some()
}

/// Register `exe` as a handler for `exts` (each with its leading dot, e.g. ".jpg").
pub fn register(layout: &Layout, exe: &Path, exts: &[String]) -> Result<(), String> {
    apply(layout, exe, exts, exts)
}

/// Make exactly `enabled` the set of types NuViewer offers to open. `all` is every type NuViewer knows, so the
/// ones that were switched off get their `OpenWithProgids` entry removed again (other programs' keys stay alone).
/// With an empty `enabled` NuViewer is still registered as an application, just for no type.
pub fn apply(layout: &Layout, exe: &Path, all: &[String], enabled: &[String]) -> Result<(), String> {
    // Start the per-type lists from scratch so a previous, larger selection can't linger.
    let exe_name = exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "nuviewer.exe".into());
    delete_tree(&format!("{}\\Capabilities\\FileAssociations", layout.app_key));
    delete_tree(&format!("{}\\Applications\\{exe_name}\\SupportedTypes", layout.classes));
    for ext in all.iter().filter(|e| !enabled.contains(e)) {
        delete_value(&format!("{}\\{ext}\\OpenWithProgids", layout.classes), PROG_ID);
    }
    let exe_str = exe.display().to_string();
    let command = format!("\"{exe_str}\" \"%1\"");
    let exe_name = exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "nuviewer.exe".into());

    let prog = format!("{}\\{PROG_ID}", layout.classes);
    set_string(&prog, None, "NuViewer image")?;
    set_string(&format!("{prog}\\DefaultIcon"), None, &format!("\"{exe_str}\",0"))?;
    set_string(&format!("{prog}\\shell\\open\\command"), None, &command)?;

    // "Open with" list entry keyed by exe name, so the app appears with a proper name even for unregistered types.
    let app = format!("{}\\Applications\\{exe_name}", layout.classes);
    set_string(&app, Some("FriendlyAppName"), APP_NAME)?;
    set_string(&format!("{app}\\shell\\open\\command"), None, &command)?;

    // Default-apps capabilities.
    let caps = format!("{}\\Capabilities", layout.app_key);
    set_string(&caps, Some("ApplicationName"), APP_NAME)?;
    set_string(&caps, Some("ApplicationDescription"), APP_DESCRIPTION)?;
    for ext in enabled {
        set_marker(&format!("{}\\{ext}\\OpenWithProgids", layout.classes), PROG_ID)?;
        set_string(&format!("{app}\\SupportedTypes"), Some(ext), "")?;
        set_string(&format!("{caps}\\FileAssociations"), Some(ext), PROG_ID)?;
    }
    set_string(&layout.registered_apps, Some(APP_NAME), &caps)?;
    Ok(())
}

/// The types NuViewer currently offers to open (each with its leading dot, lowercase), read back from the registry.
pub fn registered_exts(layout: &Layout) -> Vec<String> {
    let sub = wide(&format!("{}\\Capabilities\\FileAssociations", layout.app_key));
    let mut out = Vec::new();
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(sub.as_ptr()), None, KEY_READ, &mut key).is_err() {
            return out;
        }
        let mut index = 0u32;
        loop {
            let mut buf = [0u16; 256];
            let mut len = buf.len() as u32;
            let rc = RegEnumValueW(key, index, Some(windows::core::PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None);
            if rc.is_err() {
                break; // ERROR_NO_MORE_ITEMS (or anything else): done
            }
            out.push(String::from_utf16_lossy(&buf[..len as usize]).to_ascii_lowercase());
            index += 1;
        }
        let _ = RegCloseKey(key);
    }
    out
}

/// Which program Windows really opens `ext` (with its dot) with for this user: the ProgID Explorer stored when the
/// user picked a default (`UserChoice`), or None if they never did. Windows protects that value with a hash, so we
/// only ever read it; only the user can change it (Settings > Default apps).
pub fn user_choice(ext: &str) -> Option<String> {
    read_string(&format!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts\\{ext}\\UserChoice"), Some("ProgId"))
}

/// Remove everything `register` created. Other programs' keys (including the `.ext` keys themselves) are left alone.
pub fn unregister(layout: &Layout, exe_name: &str, exts: &[String]) {
    delete_tree(&format!("{}\\{PROG_ID}", layout.classes));
    delete_tree(&format!("{}\\Applications\\{exe_name}", layout.classes));
    for ext in exts {
        delete_value(&format!("{}\\{ext}\\OpenWithProgids", layout.classes), PROG_ID);
    }
    delete_tree(&layout.app_key);
    delete_value(&layout.registered_apps, APP_NAME);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_then_unregister_in_a_scratch_key() {
        // Never touches the real Software\Classes: everything goes under a unique scratch key that is removed at the end.
        let prefix = format!("Software\\NuViewerTest_{}", std::process::id());
        let layout = Layout::sandbox(&prefix);
        let exts: Vec<String> = [".jpg", ".png"].iter().map(|s| s.to_string()).collect();
        let exe = Path::new("C:\\Fake Dir\\nuviewer.exe");

        assert!(!is_registered(&layout));
        register(&layout, exe, &exts).unwrap();
        assert!(is_registered(&layout));

        let prog = format!("{}\\{PROG_ID}", layout.classes);
        assert_eq!(read_string(&format!("{prog}\\shell\\open\\command"), None).unwrap(), "\"C:\\Fake Dir\\nuviewer.exe\" \"%1\"");
        assert_eq!(read_string(&prog, None).unwrap(), "NuViewer image");
        assert!(value_exists(&format!("{}\\.jpg\\OpenWithProgids", layout.classes), PROG_ID));
        assert!(value_exists(&format!("{}\\.png\\OpenWithProgids", layout.classes), PROG_ID));
        assert_eq!(read_string(&format!("{}\\Capabilities\\FileAssociations", layout.app_key), Some(".png")).unwrap(), PROG_ID);
        assert_eq!(read_string(&layout.registered_apps, Some("NuViewer")).unwrap(), format!("{}\\Capabilities", layout.app_key));
        assert_eq!(read_string(&format!("{}\\Applications\\nuviewer.exe", layout.classes), Some("FriendlyAppName")).unwrap(), "NuViewer");

        // Registering twice is harmless.
        register(&layout, exe, &exts).unwrap();
        let mut back = registered_exts(&layout);
        back.sort();
        assert_eq!(back, vec![".jpg", ".png"]);

        // Narrowing the selection drops the unticked type everywhere, and keeps the ticked one.
        let only_png = vec![".png".to_string()];
        apply(&layout, exe, &exts, &only_png).unwrap();
        assert_eq!(registered_exts(&layout), vec![".png"]);
        assert!(!value_exists(&format!("{}\\.jpg\\OpenWithProgids", layout.classes), PROG_ID));
        assert!(value_exists(&format!("{}\\.png\\OpenWithProgids", layout.classes), PROG_ID));
        assert!(read_string(&format!("{}\\Applications\\nuviewer.exe\\SupportedTypes", layout.classes), Some(".jpg")).is_none());
        assert!(is_registered(&layout));

        // Nothing ticked: still a registered application, but offered for no type.
        apply(&layout, exe, &exts, &[]).unwrap();
        assert!(registered_exts(&layout).is_empty() && is_registered(&layout));
        register(&layout, exe, &exts).unwrap();

        unregister(&layout, "nuviewer.exe", &exts);
        assert!(!is_registered(&layout));
        assert!(!value_exists(&format!("{}\\.jpg\\OpenWithProgids", layout.classes), PROG_ID));
        assert!(read_string(&layout.registered_apps, Some("NuViewer")).is_none());
        delete_tree(&prefix); // remove the scratch key itself
    }
}
