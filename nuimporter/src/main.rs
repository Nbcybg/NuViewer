#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! NuImporter — MOCK-UP. Bring pictures from a memory card, a camera or a folder into the library.
//!
//! What works already: choose a source (a removable drive, a folder, or sample data so the window can be tried with no card),
//! scan it, see what it holds, choose where the pictures would go (sub-folders by date, a naming pattern, an event name) and
//! what should happen around the copy; "Import" shows exactly what it *would* do. What does not exist yet: the copy itself
//! (checksummed, resumable), the second copy, deleting from the card, ratings and keywords on the way in, the card eject.
//! The rules it is being designed around: RAW and JPG of one shot are two separate files and are never merged or skipped;
//! nothing is deleted from a card before every copy has been read back and compared.
//!
//! Started from NuGallery (Tools > Import photos…): `nuimporter.exe --from gallery --into <folder shown in the gallery>`.

mod plan;

use nv_win::ui::{Form, ID_CANCEL, SELECTION_CHANGED};
use plan::{Found, Layout, Naming};
use std::path::{Path, PathBuf};

const ID_SOURCE: u16 = 100;
const ID_SCAN: u16 = 101;
const ID_STATUS: u16 = 102;
const ID_LIST: u16 = 103;
const ID_DEST: u16 = 110;
const ID_BROWSE: u16 = 111;
const ID_LAYOUT: u16 = 112;
const ID_NAMING: u16 = 113;
const ID_EVENT: u16 = 114;
const ID_PLAN: u16 = 115;
const ID_VERIFY: u16 = 120;
const ID_SKIP_DONE: u16 = 121;
const ID_DELETE: u16 = 122;
const ID_SECOND: u16 = 123;
const ID_SECOND_DIR: u16 = 124;
const ID_RATE: u16 = 125;
const ID_OPEN: u16 = 126;
const ID_EJECT: u16 = 127;
const ID_IMPORT: u16 = 130;

/// What a source choice stands for.
enum Source {
    Drive(PathBuf),
    Folder,
    Sample,
}

fn main() {
    nv_win::crash::install();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let into = args.iter().position(|a| a == "--into").and_then(|i| args.get(i + 1)).map(PathBuf::from);
    let dest = into.filter(|p| p.is_dir()).or_else(|| nv_win::files::known_folder("pictures")).unwrap_or_else(|| PathBuf::from("."));
    if let Err(e) = run(dest) {
        nv_win::shell::message(&format!("NuImporter couldn't start: {e}"));
    }
}

fn run(dest: PathBuf) -> Result<(), String> {
    let _ = nv_win::com_init();
    // Sources: the removable drives right now, then a folder of the user's choice, then sample data.
    let mut sources: Vec<(String, Source)> = nv_win::files::drives()
        .into_iter()
        .filter(|(_, k)| *k == nv_win::files::DriveKind::Removable)
        .map(|(root, _)| (format!("Removable disk ({})", root.to_string_lossy().trim_end_matches('\\')), Source::Drive(root)))
        .collect();
    sources.push(("A folder on this PC…".to_string(), Source::Folder));
    sources.push(("Sample camera card (demo data, nothing real)".to_string(), Source::Sample));
    let source_names: Vec<&str> = sources.iter().map(|(n, _)| n.as_str()).collect();
    let first_sample = sources.len() - 1;

    let form = Form::new("NuImporter — mock-up: nothing is copied yet", 760, 640)?;
    form.label(14, 8, 732, 34, "Bring pictures from a card, camera or folder into your library. This window is a mock-up: you can try every choice, and Import shows what it would do — it copies nothing.");

    form.group(12, 46, 736, 218, "1  Import from");
    form.combo(ID_SOURCE, 28, 70, 420, 200, &source_names, if sources.len() > 2 { 0 } else { first_sample });
    form.button(ID_SCAN, 460, 69, 120, 26, "Scan", false);
    form.label_id(ID_STATUS, 28, 100, 700, 20, "Choose a source and press Scan.");
    form.list(ID_LIST, 28, 124, 704, 130);

    form.group(12, 270, 736, 150, "2  Import to");
    form.edit(ID_DEST, 28, 294, 600, 24, &dest.display().to_string());
    form.button(ID_BROWSE, 636, 293, 96, 26, "Browse…", false);
    form.label(28, 328, 110, 20, "Sub-folders:");
    form.combo(ID_LAYOUT, 140, 324, 280, 120, &Layout::ALL.map(|l| l.label()), 1);
    form.label(432, 328, 90, 20, "File names:");
    form.combo(ID_NAMING, 520, 324, 212, 120, &Naming::ALL.map(|n| n.label()), 0);
    form.label(28, 360, 110, 20, "Event name:");
    form.edit(ID_EVENT, 140, 356, 280, 24, "");
    form.label(432, 360, 300, 20, "(added to the folder name, e.g. 2025-06-12 Kyoto)");
    form.label_id(ID_PLAN, 28, 390, 704, 20, "");

    form.group(12, 426, 736, 160, "3  While importing");
    form.check(ID_VERIFY, 28, 450, 340, 22, "Read every copy back and compare it", true);
    form.check(ID_SKIP_DONE, 380, 450, 350, 22, "Skip pictures that were imported before", true);
    form.check(ID_DELETE, 28, 476, 340, 22, "Delete from the card afterwards (only after the check)", false);
    form.check(ID_SECOND, 380, 476, 150, 22, "Also copy to:", false);
    form.edit(ID_SECOND_DIR, 532, 474, 200, 24, "");
    form.check(ID_RATE, 28, 502, 340, 22, "Give them a rating / keywords (asked at the start)", false);
    form.check(ID_OPEN, 380, 502, 350, 22, "Open the folder in NuGallery when done", true);
    form.check(ID_EJECT, 28, 528, 340, 22, "Eject the card when done", false);
    form.label(380, 530, 360, 50, "RAW and JPG of one shot stay two files and are always imported together or not at all, never merged.");

    form.button(ID_IMPORT, 540, 598, 120, 30, "Import…", true);
    form.button(ID_CANCEL, 668, 598, 80, 30, "Close", false);

    let mut found: Vec<Found> = Vec::new();
    form.run(&mut |f, id| {
        match id {
            x if x == ID_SCAN || x == (ID_SOURCE | SELECTION_CHANGED) => {
                let pick = f.combo_index(ID_SOURCE).min(sources.len() - 1);
                let root = match &sources[pick].1 {
                    Source::Drive(root) => Some(root.clone()),
                    Source::Folder => nv_win::shell::pick_folder(f.hwnd, "Import from which folder?", None),
                    Source::Sample => None,
                };
                if id == ID_SCAN || matches!(sources[pick].1, Source::Sample) {
                    found = match (&sources[pick].1, root) {
                        (Source::Sample, _) => plan::sample(),
                        (_, Some(r)) => plan::scan(&r),
                        _ => Vec::new(),
                    };
                    show(f, &found);
                }
            }
            ID_BROWSE => {
                if let Some(p) = nv_win::shell::pick_folder(f.hwnd, "Import into which folder?", Some(Path::new(f.text(ID_DEST).trim()))) {
                    f.set_text(ID_DEST, &p.display().to_string());
                }
                show_plan(f, &found);
            }
            x if x == ID_LAYOUT | SELECTION_CHANGED || x == ID_NAMING | SELECTION_CHANGED => show_plan(f, &found),
            ID_IMPORT => {
                nv_win::shell::message(&dry_run(f, &found));
            }
            ID_CANCEL => return true,
            _ => {}
        }
        false
    });
    Ok(())
}

/// Put the found pictures in the list and say how many there are.
fn show(f: &Form, found: &[Found]) {
    let rows: Vec<String> = found.iter().take(2000).map(|p| format!("{}    {}    {}", p.name, plan::fmt_bytes(p.size), plan::format_time(p.taken))).collect();
    f.list_set(ID_LIST, &rows, None);
    let total: u64 = found.iter().map(|p| p.size).sum();
    f.set_text(ID_STATUS, &if found.is_empty() { "Nothing found: no pictures in that place.".to_string() } else { format!("Found {} pictures, {}.", found.len(), plan::fmt_bytes(total)) });
    show_plan(f, found);
}

/// The line under the destination: where the pictures would end up.
fn show_plan(f: &Form, found: &[Found]) {
    f.set_text(ID_PLAN, &plan::summary(found, &settings(f)));
}

fn settings(f: &Form) -> plan::Settings {
    plan::Settings { dest: PathBuf::from(f.text(ID_DEST).trim()), layout: Layout::ALL[f.combo_index(ID_LAYOUT).min(Layout::ALL.len() - 1)], naming: Naming::ALL[f.combo_index(ID_NAMING).min(Naming::ALL.len() - 1)], event: f.text(ID_EVENT).trim().to_string() }
}

/// The text of the "Import…" answer: what would be done, step by step.
fn dry_run(f: &Form, found: &[Found]) -> String {
    if found.is_empty() {
        return "Scan a source first: there is nothing to import yet.".to_string();
    }
    let s = settings(f);
    let mut out = format!("MOCK-UP — nothing was copied.\n\nIt would import {} files into {}:\n\n", found.len(), s.dest.display());
    for (folder, n, bytes) in plan::folders(found, &s).into_iter().take(8) {
        out += &format!("  {folder}    {n} files, {}\n", plan::fmt_bytes(bytes));
    }
    out += "\nThen:\n";
    let step = |on: bool, text: &str| if on { format!("  • {text}\n") } else { String::new() };
    out += &step(f.is_checked(ID_VERIFY), "read every copy back and compare it with the original");
    out += &step(f.is_checked(ID_SKIP_DONE), "skip pictures already imported before");
    out += &step(f.is_checked(ID_SECOND), &format!("make a second copy in {}", f.text(ID_SECOND_DIR).trim()));
    out += &step(f.is_checked(ID_RATE), "ask for a rating and keywords, and write them to the copies");
    out += &step(f.is_checked(ID_DELETE), "delete the originals from the card — only after every copy was compared");
    out += &step(f.is_checked(ID_OPEN), "open the folder in NuGallery");
    out += &step(f.is_checked(ID_EJECT), "eject the card");
    out += "\nRAW and JPG of the same shot stay two separate files.";
    out
}
