//! Dev tool: list / unpack an archive the way NuGallery does.   arc <archive> <dest> [password]
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (path, dest, pw) = (std::path::Path::new(&a[0]), std::path::Path::new(&a[1]), a.get(2).map(|s| s.as_str()));
    println!("list: {:?}", nv_core::archive_tools::list(path, pw, "").map(|l| l.iter().map(|e| (e.name.clone(), e.size, e.encrypted)).collect::<Vec<_>>()));
    println!("extract: {:?}", nv_core::archive_tools::extract(path, dest, &[], pw, "", &mut |_, _| true));
}
