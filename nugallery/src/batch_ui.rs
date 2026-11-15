//! The batch dialogs (rename, convert, change time stamp, remove metadata): plain Win32 forms from `nv_win::ui`, opened for
//! the selected files. They only collect the choices; the work is `nv_core::batch`, `nv_editor::convert` and
//! `nv_codecs::metastrip`, run as a job where it takes a while.

use crate::app::{App, JobResult};
use crate::ops::{self, ConvertSpec};
use nv_core::batch::{self, CaseMode, PlanItem, RenameSpec, Source, TimeRule, WhichTimes};
use nv_editor::convert::Out;
use nv_win::ui::{Form, ID_CANCEL, ID_OK};
use std::path::PathBuf;

/// The number in an edit box, or `default` when it is empty or not a number.
pub fn number(text: &str, default: i64) -> i64 {
    text.trim().parse().unwrap_or(default)
}

/// Days, hours, minutes and seconds (each may be negative) as one number of seconds.
pub fn shift_seconds(days: i64, hours: i64, minutes: i64, seconds: i64) -> i64 {
    days * 86_400 + hours * 3600 + minutes * 60 + seconds
}

/// "old name  →  new name" lines of a plan for the preview list (a problem is shown instead of the new name).
pub fn preview_lines(plan: &[PlanItem], limit: usize) -> Vec<String> {
    let name = |p: &std::path::Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut v: Vec<String> = plan
        .iter()
        .take(limit)
        .map(|p| match (&p.error, p.unchanged) {
            (Some(e), _) => format!("{}   ✗ {e}", name(&p.from)),
            (None, true) => format!("{}   (unchanged)", name(&p.from)),
            (None, false) => format!("{}   →   {}", name(&p.from), name(&p.to)),
        })
        .collect();
    if plan.len() > limit {
        v.push(format!("… and {} more", plan.len() - limit));
    }
    v
}

fn spec_from(form: &Form) -> RenameSpec {
    let mut spec = RenameSpec {
        template: form.text(10),
        find: form.text(11),
        replace: form.text(12),
        case: CaseMode::ALL.get(form.combo_index(13)).copied().unwrap_or_default(),
        start: number(&form.text(14), 1).clamp(0, 99_999_999) as u32,
        step: number(&form.text(15), 1).clamp(0, 1_000_000) as u32,
        ext_lower: form.is_checked(16),
    };
    if spec.template.trim().is_empty() {
        spec.template = "{name}".into();
    }
    spec
}

impl App {
    fn batch_files(&mut self, what: &str) -> Option<Vec<PathBuf>> {
        let v: Vec<PathBuf> = self.sel.entries(&self.listing).into_iter().filter(|e| !e.is_dir()).map(|e| e.path.clone()).collect();
        if v.is_empty() {
            self.toast(format!("Select the files to {what} first"));
            return None;
        }
        Some(v)
    }

    fn sources_for(&self, paths: &[PathBuf], need_taken: bool) -> Vec<Source> {
        let offset = nv_win::utc_offset_minutes();
        paths
            .iter()
            .map(|p| Source {
                path: p.clone(),
                modified: batch::read_file_times(p).map(|t| t.1).unwrap_or(0),
                taken: if need_taken { nv_codecs::metadata::date_taken(p).and_then(|s| batch::parse_local_datetime(&s, offset)) } else { None },
            })
            .collect()
    }

    pub fn batch_rename(&mut self) {
        let Some(paths) = self.batch_files("rename") else { return };
        let need_taken_hint = true;
        let offset = nv_win::utc_offset_minutes();
        let Ok(form) = Form::new(&format!("Rename {} files", paths.len()), 560, 470) else { return };
        form.label(14, 14, 120, 18, "Name pattern");
        form.edit(10, 134, 12, 410, 24, "{name}");
        form.label(134, 40, 410, 34, "{name} old name   {n} / {n:3} counter   {date}  {time}  {taken} date taken   {folder}");
        form.label(14, 84, 120, 18, "Find");
        form.edit(11, 134, 82, 190, 24, "");
        form.label(334, 84, 70, 18, "Replace with");
        form.edit(12, 410, 82, 134, 24, "");
        form.label(14, 120, 120, 18, "Letters");
        form.combo(13, 134, 116, 190, 200, &CaseMode::ALL.iter().map(|c| c.label()).collect::<Vec<_>>(), 0);
        form.check(16, 334, 118, 210, 20, "Make the extension lower case", false);
        form.label(14, 156, 120, 18, "Numbers start at");
        form.edit(14, 134, 152, 70, 24, "1");
        form.label(220, 156, 40, 18, "step");
        form.edit(15, 262, 152, 62, 24, "1");
        form.label(14, 192, 520, 18, "Preview (the files are numbered in the order they are shown)");
        form.list(17, 14, 214, 530, 200);
        form.button(18, 14, 424, 110, 28, "Update preview", false);
        form.button(ID_OK, 360, 424, 90, 28, "Rename", true);
        form.button(ID_CANCEL, 456, 424, 88, 28, "Cancel", false);
        let _ = need_taken_hint;
        let mut outcome: Option<Vec<Result<PathBuf, String>>> = None;
        let sources = self.sources_for(&paths, true);
        form.run(&mut |f: &Form, id: u16| {
            match id {
                18 => {
                    let plan = batch::plan_rename(&spec_from(f), &sources, offset);
                    f.list_set(17, &preview_lines(&plan, 60), None);
                    false
                }
                ID_OK => {
                    let plan = batch::plan_rename(&spec_from(f), &sources, offset);
                    let problems = plan.iter().filter(|p| p.error.is_some()).count();
                    if problems > 0 {
                        f.list_set(17, &preview_lines(&plan, 60), None);
                        nv_win::shell::message(&format!("{problems} of the new names can't be used — see the lines marked ✗ in the preview. Change the pattern and try again."));
                        return false;
                    }
                    if plan.iter().all(|p| p.unchanged) {
                        nv_win::shell::message("These settings leave every name as it is.");
                        return false;
                    }
                    outcome = Some(batch::apply_rename(&plan));
                    true
                }
                ID_CANCEL => true,
                _ => false,
            }
        });
        if let Some(results) = outcome {
            let ok: Vec<PathBuf> = results.iter().filter_map(|r| r.as_ref().ok().cloned()).collect();
            let failed = results.iter().filter(|r| r.is_err()).count();
            let first_err = results.iter().find_map(|r| r.as_ref().err().cloned());
            let mut msg = format!("Renamed {} file{}", ok.len(), if ok.len() == 1 { "" } else { "s" });
            if failed > 0 {
                msg += &format!(" — {failed} failed ({})", first_err.unwrap_or_default());
            }
            self.toast(msg);
            self.refresh(ok);
        }
    }

    pub fn batch_convert(&mut self) {
        let paths: Vec<PathBuf> = self.sel.entries(&self.listing).into_iter().filter(|e| e.viewable()).map(|e| e.path.clone()).collect();
        if paths.is_empty() {
            self.toast("Select the pictures to convert first");
            return;
        }
        let Ok(form) = Form::new(&format!("Convert {} pictures", paths.len()), 500, 330) else { return };
        form.label(14, 18, 100, 18, "Format");
        form.combo(10, 124, 14, 200, 200, &["JPEG", "PNG", "WebP (lossless)", "BMP"], 0);
        form.label(14, 52, 100, 18, "JPEG quality");
        form.edit(11, 124, 48, 60, 24, "90");
        form.label(192, 52, 200, 18, "1 - 100 (90 is a good start)");
        form.label(14, 86, 100, 18, "Longest side");
        form.edit(12, 124, 82, 80, 24, "");
        form.label(212, 86, 270, 18, "pixels (empty: keep the size)");
        form.label(14, 120, 100, 18, "Save in");
        form.edit(13, 124, 116, 280, 24, "");
        form.button(19, 410, 114, 76, 26, "Browse…", false);
        form.label(124, 146, 360, 18, "(empty: next to each original)");
        form.label(14, 176, 100, 18, "Add to the name");
        form.edit(14, 124, 172, 120, 24, "");
        form.check(15, 14, 212, 470, 20, "Send the originals to the Recycle Bin afterwards", false);
        form.label(14, 240, 470, 36, "Pictures that are already in the chosen format are written as a new file too; nothing is ever overwritten.");
        form.button(ID_OK, 300, 288, 90, 28, "Convert", true);
        form.button(ID_CANCEL, 396, 288, 90, 28, "Cancel", false);
        let mut spec: Option<ConvertSpec> = None;
        let hwnd = self.hwnd;
        form.run(&mut |f: &Form, id: u16| match id {
            19 => {
                if let Some(d) = nv_win::shell::pick_folder(f.hwnd, "Save the converted pictures in", None) {
                    f.set_text(13, &d.display().to_string());
                }
                false
            }
            ID_OK => {
                let out = match f.combo_index(10) {
                    0 => Out::Jpeg(number(&f.text(11), 90).clamp(1, 100) as u8),
                    1 => Out::Png,
                    2 => Out::Webp,
                    _ => Out::Bmp,
                };
                let max_edge = number(&f.text(12), 0);
                let dest = f.text(13).trim().trim_matches('"').to_string();
                let dest = (!dest.is_empty()).then(|| PathBuf::from(dest));
                if let Some(d) = &dest {
                    if !d.is_dir() {
                        let _ = hwnd;
                        nv_win::shell::message("The folder to save in doesn't exist.");
                        return false;
                    }
                }
                spec = Some(ConvertSpec { out, max_edge: (max_edge > 0).then_some(max_edge.min(65_535) as u32), dest, suffix: f.text(14), delete_originals: f.is_checked(15) });
                true
            }
            ID_CANCEL => true,
            _ => false,
        });
        if let Some(spec) = spec {
            self.convert_selected(spec);
        }
    }

    pub fn batch_times(&mut self) {
        let Some(paths) = self.batch_files("change the time stamp of") else { return };
        let offset = nv_win::utc_offset_minutes();
        let now = batch::format_local(batch::unix_of(std::time::SystemTime::now()), offset);
        let Ok(form) = Form::new(&format!("Change the time stamp of {} files", paths.len()), 500, 360) else { return };
        form.label(14, 16, 460, 18, "What the new time is");
        form.combo(10, 14, 38, 470, 200, &["Set every file to a date and time", "Move the time of every file earlier or later", "Take it from the picture (EXIF date taken)"], 0);
        form.label(14, 78, 120, 18, "Date and time");
        form.edit(11, 134, 74, 200, 24, &now);
        form.label(344, 78, 140, 18, "year-month-day h:m:s");
        form.label(14, 110, 460, 18, "Each file after the first gets this many seconds more (0: all the same)");
        form.edit(12, 14, 132, 80, 24, "0");
        form.label(14, 168, 460, 18, "Or move the time by (negative = earlier)");
        form.label(14, 194, 40, 18, "days");
        form.edit(13, 54, 190, 56, 24, "0");
        form.label(120, 194, 40, 18, "hours");
        form.edit(14, 164, 190, 56, 24, "0");
        form.label(230, 194, 50, 18, "minutes");
        form.edit(15, 284, 190, 56, 24, "0");
        form.label(14, 232, 100, 18, "Change");
        form.check(16, 90, 230, 110, 20, "Modified", true);
        form.check(17, 206, 230, 110, 20, "Created", true);
        form.check(18, 320, 230, 130, 20, "Last opened", false);
        form.button(ID_OK, 300, 308, 90, 28, "Apply", true);
        form.button(ID_CANCEL, 396, 308, 90, 28, "Cancel", false);
        let mut chosen: Option<(TimeRule, WhichTimes)> = None;
        form.run(&mut |f: &Form, id: u16| match id {
            ID_OK => {
                let which = WhichTimes { modified: f.is_checked(16), created: f.is_checked(17), accessed: f.is_checked(18) };
                if !(which.modified || which.created || which.accessed) {
                    nv_win::shell::message("Tick at least one of Modified, Created, Last opened.");
                    return false;
                }
                let rule = match f.combo_index(10) {
                    0 => match batch::parse_local_datetime(&f.text(11), offset) {
                        Some(unix) => TimeRule::Set { unix, step: number(&f.text(12), 0) },
                        None => {
                            nv_win::shell::message("That date isn't a real date and time. Write it like 2025-06-01 14:30:00.");
                            return false;
                        }
                    },
                    1 => TimeRule::Shift(shift_seconds(number(&f.text(13), 0), number(&f.text(14), 0), number(&f.text(15), 0), 0)),
                    _ => TimeRule::FromTaken,
                };
                chosen = Some((rule, which));
                true
            }
            ID_CANCEL => true,
            _ => false,
        });
        if let Some((rule, which)) = chosen {
            self.spawn_job("Changing time stamps", move |progress| {
                let total = paths.len();
                let (mut done, mut skipped, mut failed) = (0, 0, None);
                for (i, p) in paths.iter().enumerate() {
                    if !progress(format!("{} / {total}", i + 1)) {
                        break;
                    }
                    let current = batch::read_file_times(p).map(|t| t.1).unwrap_or(0);
                    let taken = if rule == TimeRule::FromTaken { nv_codecs::metadata::date_taken(p).and_then(|s| batch::parse_local_datetime(&s, offset)) } else { None };
                    match batch::target_time(rule, i, current, taken) {
                        None => skipped += 1,
                        Some(t) => match batch::set_file_times(p, t, which) {
                            Ok(()) => done += 1,
                            Err(e) => {
                                failed.get_or_insert(e);
                            }
                        },
                    }
                }
                let mut message = format!("Time stamp changed on {done} of {total} files");
                if skipped > 0 {
                    message += &format!(" ({skipped} have no date taken)");
                }
                if let Some(e) = failed {
                    message += &format!(" — {e}");
                }
                JobResult { message, refresh: true, select: paths.clone() }
            });
        }
    }

    pub fn batch_strip(&mut self) {
        let Some(paths) = self.batch_files("remove metadata from") else { return };
        let Ok(form) = Form::new(&format!("Remove metadata from {} files", paths.len()), 480, 250) else { return };
        form.label(14, 14, 450, 70, "Removes EXIF (camera, date, GPS position), XMP, IPTC and text comments from JPEG, PNG and WebP files. The picture itself is not touched or re-compressed, and the colour profile stays.");
        form.check(10, 14, 96, 450, 20, "Keep the orientation, so rotated photos stay upright", true);
        form.label(14, 126, 450, 36, "The original of every changed file is kept in NuViewer's undo folder for 7 days. Other formats are skipped.");
        form.button(ID_OK, 270, 196, 100, 28, "Remove", true);
        form.button(ID_CANCEL, 376, 196, 90, 28, "Cancel", false);
        let mut keep: Option<bool> = None;
        form.run(&mut |f: &Form, id: u16| match id {
            ID_OK => {
                keep = Some(f.is_checked(10));
                true
            }
            ID_CANCEL => true,
            _ => false,
        });
        if let Some(keep_orientation) = keep {
            self.spawn_job("Removing metadata", move |progress| {
                let total = paths.len();
                let (mut changed, mut clean, mut skipped, mut first_err) = (0, 0, 0, None);
                for (i, p) in paths.iter().enumerate() {
                    if !progress(format!("{} / {total}", i + 1)) {
                        break;
                    }
                    match ops::strip_one(p, keep_orientation) {
                        Ok(true) => changed += 1,
                        Ok(false) => clean += 1,
                        Err(e) => {
                            skipped += 1;
                            first_err.get_or_insert(e);
                        }
                    }
                }
                let mut message = format!("Metadata removed from {changed} file{}", if changed == 1 { "" } else { "s" });
                if clean > 0 {
                    message += &format!(", {clean} had none");
                }
                if skipped > 0 {
                    message += &format!(", {skipped} skipped ({})", first_err.unwrap_or_default());
                }
                JobResult { message, refresh: true, select: paths.clone() }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_fall_back_to_the_default() {
        assert_eq!(number(" 42 ", 1), 42);
        assert_eq!(number("", 7), 7);
        assert_eq!(number("abc", 7), 7);
        assert_eq!(number("-3", 0), -3);
    }

    #[test]
    fn shifts_add_up() {
        assert_eq!(shift_seconds(1, 2, 3, 4), 86_400 + 7200 + 180 + 4);
        assert_eq!(shift_seconds(0, -1, 0, 0), -3600);
    }

    #[test]
    fn the_preview_shows_changes_problems_and_how_many_more() {
        let plan = vec![
            PlanItem { from: "C:\\a\\x.jpg".into(), to: "C:\\a\\y.jpg".into(), unchanged: false, error: None },
            PlanItem { from: "C:\\a\\z.jpg".into(), to: "C:\\a\\z.jpg".into(), unchanged: true, error: None },
            PlanItem { from: "C:\\a\\w.jpg".into(), to: "C:\\a\\w?.jpg".into(), unchanged: false, error: Some("bad".into()) },
            PlanItem { from: "C:\\a\\v.jpg".into(), to: "C:\\a\\v2.jpg".into(), unchanged: false, error: None },
        ];
        let lines = preview_lines(&plan, 3);
        assert_eq!(lines[0], "x.jpg   →   y.jpg");
        assert!(lines[1].contains("unchanged") && lines[2].contains("✗ bad"));
        assert_eq!(lines[3], "… and 1 more");
    }
}
