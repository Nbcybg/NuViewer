//! The file-type chooser (nv-win) and the decoders (nv-codecs) must agree on which extensions exist.

#[test]
fn chooser_lists_exactly_the_decodable_extensions() {
    let mut chooser: Vec<String> = nv_win::assoc::all_exts().iter().map(|e| e.trim_start_matches('.').to_string()).collect();
    let mut codecs: Vec<String> = nv_codecs::supported_extensions().iter().map(|e| e.to_string()).collect();
    chooser.sort();
    codecs.sort();
    assert_eq!(chooser, codecs, "update FORMATS in crates/nv-win/src/assoc.rs when a decoder gains or loses an extension");
}
