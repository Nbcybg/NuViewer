//! Reading and writing the properties Windows shows in a file's Details tab (title, author, tags, comment, copyright,
//! star rating) through the shell's property system: the same code path Explorer uses, so the values land in the file's
//! own metadata (EXIF / XMP in a JPEG, TIFF tags, ...) and other programs see them. Only formats with a property handler
//! can be written (JPEG, TIFF, and a few more); the others report an error.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows::core::{GUID, PCWSTR};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Com::StructuredStorage::{InitPropVariantFromStringAsVector, PropVariantToStringAlloc, PropVariantToUInt32, PROPVARIANT};
use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreFromParsingName, GPS_DEFAULT, GPS_READWRITE};

const fn key(a: u32, b: u16, c: u16, d: [u8; 8], pid: u32) -> PROPERTYKEY {
    PROPERTYKEY { fmtid: GUID::from_values(a, b, c, d), pid }
}

const SUMMARY: (u32, u16, u16, [u8; 8]) = (0xF29F85E0, 0x4FF9, 0x1068, [0xAB, 0x91, 0x08, 0x00, 0x2B, 0x27, 0xB3, 0xD9]);
const MEDIA: (u32, u16, u16, [u8; 8]) = (0x64440492, 0x4C8B, 0x11D1, [0x8B, 0x70, 0x08, 0x00, 0x36, 0xB1, 0x1A, 0x03]);

/// The editable fields.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    Title,
    Author,
    Keywords,
    Comment,
    Copyright,
    /// 0..=5 stars.
    Rating,
}

impl Field {
    pub const ALL: [Field; 6] = [Field::Title, Field::Author, Field::Keywords, Field::Comment, Field::Copyright, Field::Rating];

    pub fn label(self) -> &'static str {
        match self {
            Field::Title => "Title",
            Field::Author => "Author",
            Field::Keywords => "Tags",
            Field::Comment => "Comment",
            Field::Copyright => "Copyright",
            Field::Rating => "Rating",
        }
    }

    fn key(self) -> PROPERTYKEY {
        let s = SUMMARY;
        let m = MEDIA;
        match self {
            Field::Title => key(s.0, s.1, s.2, s.3, 2),
            Field::Author => key(s.0, s.1, s.2, s.3, 4),
            Field::Keywords => key(s.0, s.1, s.2, s.3, 5),
            Field::Comment => key(s.0, s.1, s.2, s.3, 6),
            Field::Copyright => key(m.0, m.1, m.2, m.3, 11),
            Field::Rating => key(m.0, m.1, m.2, m.3, 9),
        }
    }

    /// Does the field hold a list (separated by `;`)?
    fn is_list(self) -> bool {
        matches!(self, Field::Author | Field::Keywords)
    }
}

/// The stored rating (0..=99, Windows' scale) for 0..=5 stars, and back.
pub fn stars_to_stored(stars: u32) -> u32 {
    match stars {
        0 => 0,
        1 => 1,
        2 => 25,
        3 => 50,
        4 => 75,
        _ => 99,
    }
}

pub fn stored_to_stars(v: u32) -> u32 {
    match v {
        0 => 0,
        1..=12 => 1,
        13..=37 => 2,
        38..=62 => 3,
        63..=87 => 4,
        _ => 5,
    }
}

thread_local! {
    static COM_READY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The shell's property handlers need COM on the calling thread (the gallery reads them on worker threads).
fn ensure_com() {
    COM_READY.with(|c| {
        if !c.get() {
            crate::com_init();
            c.set(true);
        }
    });
}

fn wide(p: &Path) -> Vec<u16> {
    p.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

/// All six fields of a file (empty strings when unset; rating as "0".."5"). None if the file has no property handler at all.
pub fn read(path: &Path) -> Option<Vec<(Field, String)>> {
    ensure_com();
    unsafe {
        let p = wide(path);
        let store: IPropertyStore = SHGetPropertyStoreFromParsingName(PCWSTR(p.as_ptr()), None, GPS_DEFAULT).ok()?;
        let mut out = Vec::new();
        for f in Field::ALL {
            let k = f.key();
            let text = match store.GetValue(&k) {
                Ok(v) => {
                    if f == Field::Rating {
                        stored_to_stars(PropVariantToUInt32(&v).unwrap_or(0)).to_string()
                    } else {
                        match PropVariantToStringAlloc(&v) {
                            Ok(s) => {
                                let t = s.to_string().unwrap_or_default();
                                CoTaskMemFree(Some(s.0 as *const _));
                                t
                            }
                            Err(_) => String::new(),
                        }
                    }
                }
                Err(_) => String::new(),
            };
            out.push((f, text));
        }
        Some(out)
    }
}

/// Write one field. Lists (author, tags) are given separated by `;`. An empty text clears the field.
pub fn write(path: &Path, field: Field, value: &str) -> Result<(), String> {
    ensure_com();
    unsafe {
        let p = wide(path);
        let store: IPropertyStore = SHGetPropertyStoreFromParsingName(PCWSTR(p.as_ptr()), None, GPS_READWRITE)
            .map_err(|e| format!("this kind of file can't hold properties here ({})", e.message().trim()))?;
        let k = field.key();
        let mut pv: PROPVARIANT = if field == Field::Rating {
            PROPVARIANT::from(stars_to_stored(value.trim().parse().unwrap_or(0)))
        } else if field.is_list() {
            let v: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
            InitPropVariantFromStringAsVector(PCWSTR(v.as_ptr())).map_err(|e| e.message().trim().to_string())?
        } else {
            PROPVARIANT::from(value)
        };
        // Shell properties are picky about the exact type; let the property system convert.
        let _ = windows::Win32::UI::Shell::PropertiesSystem::PSCoerceToCanonicalValue(&k, &mut pv);
        if value.trim().is_empty() {
            pv = PROPVARIANT::default(); // VT_EMPTY clears
        }
        store.SetValue(&k, &pv).map_err(|e| format!("can't set {}: {}", field.label(), e.message().trim()))?;
        store.Commit().map_err(|e| format!("can't save {}: {}", field.label(), e.message().trim()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rating_scale_round_trips() {
        for stars in 0..=5 {
            assert_eq!(stored_to_stars(stars_to_stored(stars)), stars);
        }
        assert_eq!(stored_to_stars(12), 1);
        assert_eq!(stored_to_stars(13), 2);
        assert_eq!(stored_to_stars(99), 5);
    }

    #[test]
    fn a_missing_file_reads_as_none() {
        assert!(read(Path::new("Z:/definitely/not/here.jpg")).is_none());
    }
}
