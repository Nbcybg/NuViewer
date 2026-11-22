//! The files NuNote (the built-in notepad) is for: plain text, Markdown, data, web, script and source files, and starting it.

use std::path::{Path, PathBuf};

/// Extensions NuNote opens: text, Markdown, data (JSON, XML, YAML, TOML, INI, CSV), web (HTML, CSS, JavaScript, TypeScript), scripts and source files.
pub const EXTS: &[&str] = &[
    "txt", "text", "log", "md", "markdown", "mdown", "mkd", "json", "jsonc", "json5", "geojson", "ndjson", "jsonl", "webmanifest", "html", "htm", "xhtml", "vue", "svelte", "xml", "xsd", "xaml", "plist", "csproj", "config", "rss", "atom", "xsl", "xslt", "resx", "css", "scss", "less", "js", "mjs", "cjs", "jsx", "ts", "tsx",
    "mts", "cts", "ini", "cfg", "conf", "properties", "env", "desktop", "toml", "yaml", "yml", "rs", "py", "pyw", "pyi", "c", "h", "cpp", "cc", "cxx", "hpp", "hh", "cs", "java", "go", "swift", "kt", "kts", "php", "dart", "scala", "sh", "bash", "zsh", "sql", "bat", "cmd", "ps1", "psm1", "psd1", "csv", "tsv", "tex", "rst",
    "srt", "vtt", "svg",
];

pub fn is_text_ext(ext: &str) -> bool {
    EXTS.contains(&ext.to_ascii_lowercase().as_str())
}

pub fn is_text_path(path: &Path) -> bool {
    path.extension().map(|e| is_text_ext(&e.to_string_lossy())).unwrap_or(false)
}

/// `nunote.exe` next to the running program, if it is there.
pub fn notepad_exe() -> Option<PathBuf> {
    std::env::current_exe().ok().map(|e| e.with_file_name("nunote.exe")).filter(|p| p.is_file())
}

/// Start NuNote on `path`. Err says why not.
pub fn open_in_notepad(path: &Path) -> Result<(), String> {
    let exe = notepad_exe().ok_or("NuNote isn't installed next to this program")?;
    std::process::Command::new(exe).arg(path).spawn().map(|_| ()).map_err(|e| format!("Couldn't start NuNote: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_files_are_recognised_by_extension() {
        for e in ["txt", "MD", "json", "html", "js", "ts", "py", "rs", "yaml", "ps1", "svg"] {
            assert!(is_text_ext(e), "{e}");
        }
        for e in ["png", "jpg", "exe", "zip", "pdf", "mp4"] {
            assert!(!is_text_ext(e), "{e}");
        }
        assert!(is_text_path(Path::new("C:\\a\\notes.txt")) && !is_text_path(Path::new("C:\\a\\photo.jpg")) && !is_text_path(Path::new("noext")));
    }
}
