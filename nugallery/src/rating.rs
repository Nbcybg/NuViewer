//! Star ratings and colour labels in the gallery: read from the XMP sidecars in the background, drawn on the tiles, changed with
//! Shift+0..5 / Shift+6..9 (the viewer uses the same keys), and used to show only what you want to see. The pictures are never
//! touched; see `nv_core::xmp`.

use crate::app::{App, Msg};
use crate::gfx::{line_h, rgb, Text};
use crate::model::Kind;
use nv_core::xmp::{self, Filter, Label, Meta, RateChange};
use nv_render::Renderer;
use std::path::PathBuf;

/// How far into a picture the gallery looks for an XMP packet (a whole folder is read, so not far).
const LOOK_BYTES: u64 = 96 * 1024;

impl App {
    /// After a folder was read: look up every picture's rating off the UI thread. Results arrive in chunks (`Msg::Ratings`).
    pub fn start_ratings(&mut self) {
        let work: Vec<(u64, PathBuf)> = self.listing.all.iter().filter(|e| matches!(e.kind, Kind::Picture | Kind::Raw)).map(|e| (e.id, e.path.clone())).collect();
        if work.is_empty() {
            return;
        }
        let (tx, wake, gen) = (self.tx.clone(), self.wake.clone(), self.scan_gen);
        std::thread::spawn(move || {
            for chunk in work.chunks(200) {
                let items: Vec<(u64, Meta)> = chunk.iter().map(|(id, p)| (*id, xmp::read_within(p, LOOK_BYTES))).filter(|(_, m)| !m.is_empty()).collect();
                if tx.send(Msg::Ratings { gen, items }).is_err() {
                    return;
                }
                wake();
            }
        });
    }

    /// A chunk of ratings arrived for the folder that is showing.
    pub fn on_ratings(&mut self, gen: u64, items: Vec<(u64, Meta)>) {
        if gen != self.scan_gen || items.is_empty() {
            return;
        }
        self.listing.ratings.extend(items);
        if self.listing.rating_filter != Filter::All {
            self.rearrange_keeping_focus();
        } else {
            nv_win::invalidate(self.hwnd);
        }
    }

    /// Rate or label the selected pictures (the focused one when nothing is selected). A grouped RAW + JPG tile gets it on both
    /// files. With several pictures selected they all take over what the focused one becomes.
    pub fn rate_selected(&mut self, change: RateChange) {
        let picked: Vec<(u64, PathBuf)> = self.sel.entries(&self.listing).into_iter().filter(|e| matches!(e.kind, Kind::Picture | Kind::Raw)).map(|e| (e.id, e.path.clone())).collect();
        let Some(first) = self.sel.cursor.and_then(|c| self.listing.get(c)).filter(|e| matches!(e.kind, Kind::Picture | Kind::Raw)).map(|e| e.id).or(picked.first().map(|p| p.0)) else {
            self.toast("Nothing to rate here");
            return;
        };
        let base = self.listing.ratings.get(&first).copied().unwrap_or_else(|| picked.iter().find(|p| p.0 == first).map(|p| xmp::read(&p.1)).unwrap_or_default());
        let new = base.changed(change);
        let mut failed = None;
        let mut done = 0;
        for (id, path) in &picked {
            let now = self.listing.ratings.get(id).copied().unwrap_or_else(|| xmp::read(path));
            let after = now.follow(new, change);
            let mut files = vec![path.clone()];
            files.extend(self.listing.pair_files(*id));
            for f in files {
                match xmp::write(&f, &after) {
                    Ok(_) => {}
                    Err(e) => failed = Some(e),
                }
            }
            if after.is_empty() {
                self.listing.ratings.remove(id);
            } else {
                self.listing.ratings.insert(*id, after);
            }
            done += 1;
        }
        if self.listing.rating_filter != Filter::All {
            self.rearrange_keeping_focus();
        }
        match failed {
            Some(e) => self.toast(format!("[NV-7008] Rating not saved: {e}")),
            None if done == 1 => self.toast(format!("{}  —  {}", picked[0].1.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), new.describe())),
            None => self.toast(format!("{done} pictures  —  {}", new.describe())),
        }
        nv_win::invalidate(self.hwnd);
    }

    /// Run a plug-in's command on the selected files (the ones it applies to).
    pub fn run_plugin(&mut self, pi: usize, ci: usize) {
        let loaded = nv_core::plugins::current();
        let files: Vec<PathBuf> = self.sel.entries(&self.listing).into_iter().filter(|e| !e.is_dir()).map(|e| e.path.clone()).collect();
        let Some((p, c)) = loaded.plugins.get(pi).and_then(|p| p.commands.get(ci).map(|c| (p, c))) else { return };
        match nv_core::plugins::run_command(p, c, &files) {
            Ok(n) => self.toast(format!("{}: {}{}", p.name, c.name, if n > 1 { format!(" ({n} started)") } else { String::new() })),
            Err(e) => self.toast(e),
        }
    }

    /// Show only the pictures that pass `f` (View > Rating).
    pub fn set_rating_filter(&mut self, f: Filter) {
        self.listing.rating_filter = f;
        self.rearrange_keeping_focus();
        self.toast(if f == Filter::All { "Showing every rating".to_string() } else { format!("Showing: {}", f.label()) });
    }

    /// The rating keys: Shift+0..5 stars, Shift+6..9 and Shift+` colour labels, Ctrl+Shift+X reject, Ctrl+Shift+` no label.
    /// True if the key was one of them.
    pub fn rating_key(&mut self, vk: u32, ctrl: bool, shift: bool, alt: bool) -> bool {
        if alt || !shift {
            return false;
        }
        let change = match vk {
            0x30..=0x35 if !ctrl => RateChange::Stars((vk - 0x30) as u8),
            0x36 if !ctrl => RateChange::Label(Some(Label::Red)),
            0x37 if !ctrl => RateChange::Label(Some(Label::Yellow)),
            0x38 if !ctrl => RateChange::Label(Some(Label::Green)),
            0x39 if !ctrl => RateChange::Label(Some(Label::Blue)),
            0xC0 if !ctrl => RateChange::Label(Some(Label::Purple)),
            0xC0 => RateChange::Label(None),
            0x58 if ctrl => RateChange::ToggleReject,
            _ => return false,
        };
        self.rate_selected(change);
        true
    }
}

/// Stars in the lower left of a thumbnail and the colour label as a dot at the upper left; a rejected picture is dimmed by a cross.
pub fn draw_badge(r: &mut Renderer, text: &mut Text, b: [f32; 4], m: Meta, d: f32) {
    let big = (b[2] - b[0]) >= 70.0 * d;
    if let Some(l) = m.label {
        let s = (if big { 12.0 } else { 8.0 }) * d;
        let (x, y) = (b[0] + 6.0 * d, b[1] + 6.0 * d);
        let _ = r.fill_rect([x - 1.0, y - 1.0, x + s + 1.0, y + s + 1.0], rgb(0x000000, 0.6));
        let _ = r.fill_rect([x, y, x + s, y + s], rgb(l.rgb(), 1.0));
    }
    let s = m.stars_text();
    if s.is_empty() {
        return;
    }
    let font = (if big { 13.0 } else { 9.0 } * d) as i32;
    let w = text.width(&s, font) as f32 + 8.0 * d;
    let h = line_h(font) + 2.0 * d;
    let (x, y) = (b[0] + 4.0 * d, b[3] - h - 4.0 * d);
    let _ = r.fill_rect([x, y, x + w, y + h], rgb(0x000000, 0.7));
    text.draw(r, &s, font, x + 4.0 * d, y + d, if m.rejected { rgb(0xE5484D, 1.0) } else { rgb(0xF5C518, 1.0) });
}
