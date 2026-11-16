//! Dev tool: writes the table of supported file types as a spreadsheet. `cargo run -p nv-codecs --example formatscsv [out.csv]`
//! (default `docs/formats.csv`).
fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| format!("{}/../../docs/formats.csv", env!("CARGO_MANIFEST_DIR")));
    let csv = nv_codecs::formats::csv();
    std::fs::write(&out, csv.as_bytes()).expect("write csv");
    println!("{} file types -> {out}", nv_codecs::formats::rows().len());
}
