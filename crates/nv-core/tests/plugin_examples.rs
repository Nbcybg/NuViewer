//! The example plug-ins in `docs/plugin-examples` must stay valid for the interface they document.

use nv_core::plugins;
use std::path::Path;

#[test]
fn the_documented_examples_load() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("docs").join("plugin-examples");
    let l = plugins::scan(&dir);
    assert!(l.skipped.is_empty(), "{:?}", l.skipped);
    let ids: Vec<&str> = l.plugins.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["imagemagick-decoder", "open-in-paint"]);
    assert!(l.plugins[0].decoder.is_some() && l.plugins[1].commands.len() == 1);
    assert_eq!(l.commands_for(Path::new("a.jpg")).len(), 1);
}
