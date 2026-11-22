# rawler (LGPL-2.1) and what it means for NuViewer

NuViewer reads camera RAW sensor data with the **`rawler`** crate (version 0.8, the RAW engine of the DNGLab project,
<https://github.com/dnglab/dnglab>), which is licensed under the **GNU Lesser General Public License 2.1**. All other libraries are MIT,
Apache-2.0, BSD, Zlib or similar (`dependencies.csv`).

* NuViewer's own code is MIT (`LICENSE`). The LGPL part is `rawler` only; it is not changed by NuViewer.
* `rawler` is linked into `nuviewer.exe`, `nugallery.exe` and the other programs that decode RAW files. The LGPL asks that you can replace
  that library with a modified one: **the complete source of NuViewer is published**, so you can do exactly that: put your own copy
  of the crate in place with a `[patch.crates-io]` entry in `Cargo.toml` (or change `crates/nv-codecs/Cargo.toml`) and run
  `cargo build --release`.
* The LGPL text: <https://www.gnu.org/licenses/old-licenses/lgpl-2.1.txt>.

This note is the developer's reading of the licence, not legal advice. If you redistribute NuViewer builds, keep this file and
`dependencies.csv` with them.
