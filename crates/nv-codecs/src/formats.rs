//! The table of every file type NuViewer opens: full format name, kind, how it is shown, whether NuEditor can edit it, and how it is read.
//! `csv()` writes it as a spreadsheet (`docs/formats.csv`, made by `cargo run -p nv-codecs --example formatscsv`); a test keeps it in step
//! with the extension lists of the decoders, so a new format cannot be added without a row here.
//!
//! Columns of a row: extension | full name | type | how NuViewer shows it | NuEditor | read by | notes
//! How shown: View (a picture) / Animation / Play (video, sound) / 3D (a model you can turn) / Document (pages) / Preview (the picture stored
//! inside) / Card (an information card, no pixels).
//! NuEditor: Yes (raster picture) / Flattened (layers, pages, first frame only) / Camera (edits kept beside a RAW file, which is never
//! rewritten) / Rasterised (vector or page drawn to pixels first; edits make a new raster file) / No (nothing to edit).

const TABLE: &str = "\
jpg|JPEG (Joint Photographic Experts Group) / JFIF|Photo|View|Yes|built-in decoder|Damaged files are repaired or read by Windows; CMYK with profile
jpeg|JPEG (Joint Photographic Experts Group)|Photo|View|Yes|built-in decoder|same as .jpg
jpe|JPEG (Joint Photographic Experts Group)|Photo|View|Yes|built-in decoder|same as .jpg
jfif|JPEG File Interchange Format|Photo|View|Yes|built-in decoder|same as .jpg
png|Portable Network Graphics|Web / lossless|View|Yes|built-in decoder|8 and 16 bit, ICC profile
apng|Animated Portable Network Graphics|Web / lossless|Animation|Flattened|built-in decoder|first frame in NuEditor
gif|Graphics Interchange Format|Web|Animation|Flattened|built-in decoder|first frame in NuEditor
webp|WebP (Google)|Web|Animation|Yes|built-in decoder|lossy, lossless, animated
avif|AV1 Image File Format|Web / photo|View|Yes|Windows codec (WIC)|needs the AV1 extension on some PCs
heic|High Efficiency Image Container (HEVC)|Photo (phone)|View|Yes|Windows codec (WIC)|needs the HEVC extension
heif|High Efficiency Image File Format|Photo (phone)|View|Yes|Windows codec (WIC)|needs the HEVC extension
hif|High Efficiency Image File (Canon / Fujifilm HEIF)|Photo (camera)|View|Yes|Windows codec (WIC)|needs the HEVC extension
jxl|JPEG XL|Web / photo|View|Yes|built-in decoder|
jp2|JPEG 2000 (JP2)|Photo / archive|View|Yes|built-in decoder|
j2k|JPEG 2000 codestream|Photo / archive|View|Yes|built-in decoder|
j2c|JPEG 2000 codestream|Photo / archive|View|Yes|built-in decoder|
jpf|JPEG 2000 part 2 (JPF)|Photo / archive|View|Yes|built-in decoder|
jpx|JPEG 2000 extended (JPX)|Photo / archive|View|Yes|built-in decoder|
jpc|JPEG 2000 codestream|Photo / archive|View|Yes|built-in decoder|
jxr|JPEG XR (HD Photo)|Photo|View|Yes|Windows codec (WIC)|
wdp|Windows Media Photo (JPEG XR)|Photo|View|Yes|Windows codec (WIC)|
hdp|HD Photo (JPEG XR)|Photo|View|Yes|Windows codec (WIC)|
flif|Free Lossless Image Format|Web / lossless|View|Yes|built-in decoder|
bmp|Windows Bitmap|Raster (Windows)|View|Yes|Windows codec (WIC)|
dib|Device-Independent Bitmap|Raster (Windows)|View|Yes|built-in decoder|bare DIB or a BMP without its file header
tif|Tagged Image File Format|Raster (print / scan)|View|Yes|Windows codec (WIC)|first page; multi-page TIFF shows page 1
tiff|Tagged Image File Format|Raster (print / scan)|View|Yes|Windows codec (WIC)|first page
ico|Windows Icon|Icon|View|Yes|Windows codec, own fallback|largest size
cur|Windows Cursor|Icon|View|Yes|built-in decoder|largest size
icns|Apple Icon Image|Icon|View|Yes|built-in decoder|
pcx|ZSoft PC Paintbrush|Raster (DOS)|View|Yes|built-in decoder|
dcx|DCX multi-page PCX (fax)|Raster (DOS / fax)|View|Flattened|built-in decoder|page 1
tga|Truevision TGA (TARGA)|Raster (games / video)|View|Yes|built-in decoder|
icb|Truevision TGA (ICB)|Raster (games / video)|View|Yes|built-in decoder|old name of TGA
vda|Truevision TGA (VDA)|Raster (games / video)|View|Yes|built-in decoder|old name of TGA
vst|Truevision TGA (VST)|Raster (games / video)|View|Yes|built-in decoder|old name of TGA
dds|DirectDraw Surface|Texture (games)|View|Yes|built-in decoder|BC1-BC7 block-compressed textures included
ppm|Portable Pixmap (Netpbm)|Raster (Unix)|View|Yes|built-in decoder|
pgm|Portable Graymap (Netpbm)|Raster (Unix)|View|Yes|built-in decoder|
pbm|Portable Bitmap (Netpbm)|Raster (Unix)|View|Yes|built-in decoder|
pam|Portable Arbitrary Map (Netpbm)|Raster (Unix)|View|Yes|built-in decoder|
pnm|Portable Anymap (Netpbm)|Raster (Unix)|View|Yes|built-in decoder|
qoi|Quite OK Image|Web / lossless|View|Yes|built-in decoder|
hdr|Radiance RGBE (High Dynamic Range)|HDR|View|Yes|built-in decoder|tone-mapped to sRGB
rgbe|Radiance RGBE (High Dynamic Range)|HDR|View|Yes|built-in decoder|tone-mapped to sRGB
exr|OpenEXR (Industrial Light & Magic)|HDR / VFX|View|Yes|built-in decoder|tone-mapped to sRGB
pcd|Kodak Photo CD|Photo (scan)|View|Yes|built-in decoder|768 x 512 base resolution
cin|Cineon Digital Film|Film frame|View|Yes|built-in decoder|10-bit log converted to display; 8 / 16 bit too
dpx|Digital Picture Exchange (SMPTE 268M)|Film frame|View|Yes|built-in decoder|RGB / RGBA / grey, 8 / 10 / 16 bit; YCbCr and RLE not supported
dcm|DICOM (medical imaging)|Scientific / medical|View|Yes|built-in decoder|first image
dicom|DICOM (medical imaging)|Scientific / medical|View|Yes|built-in decoder|first image
ecw|Enhanced Compressed Wavelet (ERDAS)|Scientific / maps|View|Yes|Windows codec if installed|needs its maker's codec; else a message
psd|Adobe Photoshop Document|Layered|View|Flattened|built-in decoder|merged picture; layers are not editable
psb|Adobe Photoshop Large Document|Layered|View|Flattened|built-in decoder|merged picture
pdn|Paint.NET Image|Layered|Preview|Flattened|built-in decoder|stored preview (up to 256 px, enlarged); layers not read
sai|PaintTool SAI Document (version 1)|Layered|View|Flattened|built-in decoder|canvas from raster layers; linework and masks not drawn
clip|Clip Studio Paint Document|Layered|Preview|Flattened|built-in decoder|canvas preview stored in the file
lip|Clip Studio (older) file|Layered|Preview|Flattened|built-in decoder|the picture found inside, else a card; format unconfirmed
oci|openCanvas Image|Layered|Card|No|built-in reader|size only; the pixel data is a closed format
cpt|Corel PHOTO-PAINT Image|Layered|Card|No|built-in reader|closed format; export PNG / TIFF from PHOTO-PAINT
afphoto|Affinity Photo Document|Layered|Preview|Flattened|built-in reader|stored preview
afdesign|Affinity Designer Document|Vector / layered|Preview|Flattened|built-in reader|stored preview
afpub|Affinity Publisher Document|Layout|Preview|Flattened|built-in reader|stored preview
aftemplate|Affinity Template|Layout|Preview|Flattened|built-in reader|stored preview
clp|Windows Clipboard File|Clipboard|View|Yes|built-in decoder|the bitmap (DIB) format inside
wal|Quake II Texture|Texture (games)|View|Yes|built-in decoder|coloured from colormap.pcx beside it, else grey
wad|WAD texture archive (Half-Life, Quake, Doom)|Texture archive (games)|View|No|built-in decoder|a contact sheet of the textures / flats / sprites inside
cr2|Canon Raw 2|Camera RAW|View|Camera|built-in (embedded JPEG or developed)|
cr3|Canon Raw 3|Camera RAW|View|Camera|built-in (embedded JPEG or developed)|
crw|Canon Raw (CIFF)|Camera RAW|View|Camera|built-in (embedded JPEG or developed)|
nef|Nikon Electronic Format|Camera RAW|View|Camera|built-in (embedded JPEG or developed)|
nrw|Nikon Raw (Coolpix)|Camera RAW|View|Camera|built-in (embedded JPEG or developed)|
arw|Sony Alpha Raw|Camera RAW|View|Camera|built-in (embedded JPEG or developed)|
sr2|Sony Raw 2|Camera RAW|View|Camera|built-in (embedded JPEG or developed)|
srf|Sony Raw Format|Camera RAW|View|Camera|built-in (embedded JPEG or developed)|
dng|Adobe Digital Negative|Camera RAW|View|Camera|built-in (embedded JPEG or developed)|
orf|Olympus Raw Format|Camera RAW|View|Camera|built-in (embedded JPEG or developed)|
rw2|Panasonic Raw 2|Camera RAW|View|Camera|built-in (embedded JPEG or developed)|
rwl|Leica Raw (Panasonic RW2 layout)|Camera RAW|View|Camera|built-in (embedded JPEG or developed)|
raf|Fujifilm Raw|Camera RAW|View|Camera|built-in (embedded JPEG or developed)|
pef|Pentax Electronic Format|Camera RAW|View|Camera|built-in (embedded JPEG or developed)|
srw|Samsung Raw|Camera RAW|View|Camera|built-in (embedded JPEG or developed)|
3fr|Hasselblad 3F Raw|Camera RAW|View|Camera|built-in (embedded JPEG or developed)|
erf|Epson Raw Format|Camera RAW|View|Camera|built-in (embedded JPEG or developed)|
kdc|Kodak Digital Camera Raw|Camera RAW|View|Camera|built-in (embedded JPEG or developed)|
mrw|Minolta Raw|Camera RAW|View|Camera|built-in (embedded JPEG or developed)|
raw|Generic camera raw (Leica / Panasonic / Kodak)|Camera RAW|View|Camera|built-in (embedded JPEG or developed)|
iiq|Phase One Intelligent Image Quality Raw|Camera RAW|View|Camera|built-in (embedded JPEG or developed)|
x3f|Sigma X3F (Foveon) Raw|Camera RAW|View|Camera|built-in (embedded preview)|
svg|Scalable Vector Graphics|Vector|View|Rasterised|resvg|
svgz|Scalable Vector Graphics (gzip)|Vector|View|Rasterised|resvg|
emf|Enhanced Metafile|Vector (clipart)|View|Rasterised|Windows GDI+|
wmf|Windows Metafile|Vector (clipart)|View|Rasterised|Windows GDI+|placeable and plain
eps|Encapsulated PostScript|Vector (print)|View|Rasterised|Ghostscript if installed, else the preview inside|
ps|PostScript|Vector (print)|View|Rasterised|Ghostscript if installed, else the preview inside|
ai|Adobe Illustrator Artwork|Vector (print)|Document|Rasterised|Windows PDF renderer (PDF-compatible files)|files saved without PDF compatibility are read as PostScript
pdf|Portable Document Format|Document|Document|Rasterised|Windows PDF renderer|page by page
dxf|AutoCAD Drawing Exchange Format|CAD / vector|3D|Rasterised|built-in reader|3D faces become a model; a flat drawing is drawn as lines
dwg|AutoCAD Drawing|CAD|3D|No|converter (ODA / Blender / FreeCAD) if installed|else a message
hpgl|HP Graphics Language plot|Vector (plotter)|View|Rasterised|built-in reader|pen moves, circles, arcs; text not drawn
hgl|HP Graphics Language plot|Vector (plotter)|View|Rasterised|built-in reader|
hpg|HP Graphics Language plot|Vector (plotter)|View|Rasterised|built-in reader|
plt|HP-GL plot file|Vector (plotter)|View|Rasterised|built-in reader|other programs also use .plt (palettes): those give a message
cgm|Computer Graphics Metafile|Vector (clipart)|View|Rasterised|built-in reader|binary encoding; clear-text encoding not supported; text not drawn
swf|Adobe Flash movie (Shockwave Flash)|Animation / web|Preview|Flattened|built-in reader|largest bitmap inside, else a card
fla|Adobe Flash Animate project|Animation / authoring|Preview|Flattened|built-in reader|a library picture, else a card
blend|Blender project|3D scene|Preview|No|built-in reader|stored preview picture
blend1|Blender project (backup)|3D scene|Preview|No|built-in reader|stored preview picture
stl|Stereolithography model|3D model|3D|No|built-in reader|
obj|Wavefront OBJ model|3D model|3D|No|built-in reader|with MTL textures
ply|Polygon File Format (Stanford)|3D model|3D|No|built-in reader|
glb|glTF Binary|3D model|3D|No|built-in reader|Draco / meshopt not supported
gltf|glTF (GL Transmission Format)|3D model|3D|No|built-in reader|
vrm|VRM avatar (glTF)|3D model|3D|No|built-in reader|
fbx|Autodesk FBX|3D model|3D|No|built-in reader|best effort
3ds|3D Studio mesh|3D model|3D|No|built-in reader|
3mf|3D Manufacturing Format|3D model (printing)|3D|No|built-in reader|slicer projects (Bambu / Orca / Prusa) included
amf|Additive Manufacturing File|3D model (printing)|3D|No|built-in reader|
dae|COLLADA Digital Asset Exchange|3D model|3D|No|built-in reader|
las|LAS point cloud|3D (survey)|3D|No|built-in reader|
lwo|LightWave Object|3D model|3D|No|built-in reader|
lwo2|LightWave Object 2|3D model|3D|No|built-in reader|
usda|Universal Scene Description (ASCII)|3D scene|3D|No|built-in reader|
usdz|Universal Scene Description (zip)|3D scene|3D|No|built-in reader|
ma|Autodesk Maya ASCII|3D scene|3D|No|built-in reader|
unitypackage|Unity package|3D / game assets|Preview|No|built-in reader|
step|STEP (ISO 10303) CAD model|CAD|3D|No|converter (FreeCAD / Blender / assimp) if installed|else a message
stp|STEP (ISO 10303) CAD model|CAD|3D|No|converter (FreeCAD / Blender / assimp) if installed|else a message
iges|IGES CAD model|CAD|3D|No|converter if installed|else a message
igs|IGES CAD model|CAD|3D|No|converter if installed|else a message
abc|Alembic|3D scene|3D|No|converter if installed|else a message
usd|Universal Scene Description|3D scene|3D|No|converter if installed|else a message
usdc|Universal Scene Description (binary)|3D scene|3D|No|converter if installed|else a message
max|Autodesk 3ds Max scene|3D scene|Preview|No|built-in reader|small stored preview, enlarged
c4d|Maxon Cinema 4D scene|3D scene|Preview|No|built-in reader|stored preview, else a message
lxo|Foundry Modo scene|3D scene|Preview|No|built-in reader|stored preview, else a message
ztl|Pixologic ZBrush tool|3D sculpt|Preview|No|built-in reader|stored preview, else a message
zpr|Pixologic ZBrush project|3D sculpt|Preview|No|built-in reader|stored preview, else a message
spp|Adobe Substance Painter project|3D texturing|Preview|No|built-in reader|stored preview, else a message
ms|Marmoset Toolbag scene|3D scene|Preview|No|built-in reader|stored preview, else a message
hrc|Softimage scene|3D scene|Preview|No|built-in reader|stored preview, else a message
uasset|Unreal Engine asset|3D / game assets|Preview|No|built-in reader|stored preview, else a message
mb|Autodesk Maya Binary|3D scene|Preview|No|built-in reader|stored preview, else a message
f3d|Autodesk Fusion 360 design|CAD|Preview|No|built-in reader|stored preview, else a message
duf|Daz Studio User File|3D scene|Preview|No|built-in reader|the .duf.png beside it, else a card from the JSON
mp4|MPEG-4 Part 14 video|Video|Play|No|Media Foundation (ffmpeg copy if needed)|poster frame in the gallery
m4v|iTunes video (MPEG-4)|Video|Play|No|Media Foundation|
mov|Apple QuickTime Movie|Video|Play|No|Media Foundation (ffmpeg copy if needed)|
mkv|Matroska Video|Video|Play|No|Media Foundation or ffmpeg copy|
avi|Audio Video Interleave|Video|Play|No|Media Foundation|
wmv|Windows Media Video|Video|Play|No|Media Foundation|
webm|WebM Video|Video|Play|No|Media Foundation or ffmpeg copy|
mpg|MPEG-1 / MPEG-2 video|Video|Play|No|Media Foundation|
mpeg|MPEG-1 / MPEG-2 video|Video|Play|No|Media Foundation|
mpe|MPEG video|Video|Play|No|Media Foundation|
ts|MPEG transport stream|Video|Play|No|Media Foundation|
mts|AVCHD video|Video|Play|No|Media Foundation|
m2ts|Blu-ray / AVCHD video|Video|Play|No|Media Foundation|
m2t|HDV video|Video|Play|No|Media Foundation|
3gp|3GPP mobile video|Video|Play|No|Media Foundation|
3g2|3GPP2 mobile video|Video|Play|No|Media Foundation|
flv|Flash Video|Video|Play|No|ffmpeg copy|needs ffmpeg
f4v|Flash MP4 Video|Video|Play|No|ffmpeg copy|needs ffmpeg
ogv|Ogg Video|Video|Play|No|ffmpeg copy|needs ffmpeg
ogm|Ogg Media|Video|Play|No|ffmpeg copy|needs ffmpeg
vob|DVD Video Object|Video|Play|No|ffmpeg copy|needs ffmpeg
asf|Advanced Systems Format|Video|Play|No|Media Foundation|
dv|Digital Video (DV)|Video|Play|No|ffmpeg copy|needs ffmpeg
rm|RealMedia|Video|Play|No|ffmpeg copy|needs ffmpeg
rmvb|RealMedia Variable Bitrate|Video|Play|No|ffmpeg copy|needs ffmpeg
divx|DivX video|Video|Play|No|ffmpeg copy|needs ffmpeg
m2v|MPEG-2 video elementary stream|Video|Play|No|ffmpeg copy|needs ffmpeg
mod|JVC / Panasonic camcorder video|Video|Play|No|ffmpeg copy|needs ffmpeg
tod|JVC camcorder video|Video|Play|No|ffmpeg copy|needs ffmpeg
wtv|Windows Recorded TV|Video|Play|No|ffmpeg copy|needs ffmpeg
r3d|RED R3D raw video|Video (cinema)|Play|No|proxy file beside it|plays a same-name .mp4 / .mov proxy; the original has no Windows codec
mxf|Material Exchange Format|Video (broadcast)|Play|No|proxy file beside it|
braw|Blackmagic RAW video|Video (cinema)|Play|No|proxy file beside it|
wav|Waveform Audio|Audio|Play|No|built-in waveform, Media Foundation|waveform picture
mp3|MPEG-1 Audio Layer III|Audio|Play|No|Media Foundation|waveform picture
flac|Free Lossless Audio Codec|Audio|Play|No|Media Foundation|
aac|Advanced Audio Coding|Audio|Play|No|Media Foundation|
m4a|MPEG-4 Audio|Audio|Play|No|Media Foundation|
m4b|MPEG-4 Audiobook|Audio|Play|No|Media Foundation|
ogg|Ogg Vorbis audio|Audio|Play|No|ffmpeg copy|needs ffmpeg
oga|Ogg audio|Audio|Play|No|ffmpeg copy|needs ffmpeg
opus|Opus audio|Audio|Play|No|ffmpeg copy|needs ffmpeg
wma|Windows Media Audio|Audio|Play|No|Media Foundation|
aif|Audio Interchange File Format|Audio|Play|No|Media Foundation|
aiff|Audio Interchange File Format|Audio|Play|No|Media Foundation|
aifc|Audio Interchange File Format (compressed)|Audio|Play|No|Media Foundation|
au|Sun / NeXT audio (.au)|Audio|Play|No|ffmpeg copy|needs ffmpeg
snd|Sun / NeXT audio (.snd)|Audio|Play|No|ffmpeg copy|needs ffmpeg
pcm|Raw PCM audio|Audio|Play|No|ffmpeg copy|needs ffmpeg
alac|Apple Lossless Audio|Audio|Play|No|ffmpeg copy|needs ffmpeg
ape|Monkey's Audio|Audio|Play|No|ffmpeg copy|needs ffmpeg
dsf|DSD Stream File|Audio|Play|No|ffmpeg copy|needs ffmpeg
dff|DSDIFF audio|Audio|Play|No|ffmpeg copy|needs ffmpeg
dsd|DSD audio|Audio|Play|No|ffmpeg copy|needs ffmpeg
wv|WavPack|Audio|Play|No|ffmpeg copy|needs ffmpeg
tta|True Audio|Audio|Play|No|ffmpeg copy|needs ffmpeg
mka|Matroska Audio|Audio|Play|No|ffmpeg copy|needs ffmpeg
mpc|Musepack|Audio|Play|No|ffmpeg copy|needs ffmpeg
mp2|MPEG-1 Audio Layer II|Audio|Play|No|Media Foundation|
amr|Adaptive Multi-Rate audio|Audio|Play|No|ffmpeg copy|needs ffmpeg
ac3|Dolby Digital (AC-3)|Audio|Play|No|ffmpeg copy|needs ffmpeg
dts|DTS audio|Audio|Play|No|ffmpeg copy|needs ffmpeg
caf|Core Audio Format|Audio|Play|No|ffmpeg copy|needs ffmpeg
weba|WebM Audio|Audio|Play|No|ffmpeg copy|needs ffmpeg
mid|Standard MIDI File|Music (notes)|Play|No|built-in synthesizer|piano roll picture; plays through a simple built-in synth
midi|Standard MIDI File|Music (notes)|Play|No|built-in synthesizer|
rmi|RIFF-wrapped MIDI|Music (notes)|Play|No|built-in synthesizer|
kar|Karaoke MIDI|Music (notes)|Play|No|built-in synthesizer|
html|HyperText Markup Language page|Web page|Preview|Rasterised|Microsoft Edge / Chrome (headless)|drawn offline; text card if no browser
htm|HyperText Markup Language page|Web page|Preview|Rasterised|Microsoft Edge / Chrome (headless)|
xhtml|XHTML page|Web page|Preview|Rasterised|Microsoft Edge / Chrome (headless)|
mhtml|MIME HTML web archive|Web page|Preview|Rasterised|Microsoft Edge / Chrome (headless)|
mht|MIME HTML web archive|Web page|Preview|Rasterised|Microsoft Edge / Chrome (headless)|
zip|ZIP archive|Archive|Preview|No|built-in reader|first picture inside, else a card
cbz|Comic book archive (ZIP)|Archive (comics)|Preview|No|built-in reader|first page picture
epub|EPUB e-book|Archive (e-book)|Preview|No|built-in reader|cover picture
7z|7-Zip archive|Archive|Card|No|card (NuGallery opens it with 7-Zip)|
cb7|Comic book archive (7z)|Archive (comics)|Card|No|card (NuGallery opens it with 7-Zip)|
rar|RAR archive|Archive|Card|No|card (NuGallery opens it with 7-Zip)|
cbr|Comic book archive (RAR)|Archive (comics)|Card|No|card (NuGallery opens it with 7-Zip)|
tar|Tape archive|Archive|Card|No|card|
tgz|Gzip-compressed tar archive|Archive|Card|No|card|
gz|Gzip file|Archive|Card|No|card|
bz2|Bzip2 file|Archive|Card|No|card|
xz|XZ file|Archive|Card|No|card|
zst|Zstandard file|Archive|Card|No|card|
docx|Word document (Open XML)|Office document|Preview|No|built-in reader|stored thumbnail, else a card
docm|Word macro-enabled document|Office document|Preview|No|built-in reader|stored thumbnail, else a card
dotx|Word template|Office document|Preview|No|built-in reader|stored thumbnail, else a card
xlsx|Excel workbook (Open XML)|Office document|Preview|No|built-in reader|stored thumbnail, else a card
xlsm|Excel macro-enabled workbook|Office document|Preview|No|built-in reader|stored thumbnail, else a card
xltx|Excel template|Office document|Preview|No|built-in reader|stored thumbnail, else a card
pptx|PowerPoint presentation (Open XML)|Office document|Preview|No|built-in reader|stored thumbnail, else a card
pptm|PowerPoint macro-enabled presentation|Office document|Preview|No|built-in reader|stored thumbnail, else a card
potx|PowerPoint template|Office document|Preview|No|built-in reader|stored thumbnail, else a card
ppsx|PowerPoint slide show|Office document|Preview|No|built-in reader|stored thumbnail, else a card
odt|OpenDocument Text|Office document|Preview|No|built-in reader|stored thumbnail, else a card
ods|OpenDocument Spreadsheet|Office document|Preview|No|built-in reader|stored thumbnail, else a card
odp|OpenDocument Presentation|Office document|Preview|No|built-in reader|stored thumbnail, else a card
odg|OpenDocument Drawing|Office document|Preview|No|built-in reader|stored thumbnail, else a card
ott|OpenDocument Text template|Office document|Preview|No|built-in reader|stored thumbnail, else a card
ots|OpenDocument Spreadsheet template|Office document|Preview|No|built-in reader|stored thumbnail, else a card
otp|OpenDocument Presentation template|Office document|Preview|No|built-in reader|stored thumbnail, else a card
txt|Plain text|Text|Card|No|built-in reader|first lines on a page
md|Markdown text|Text|Card|No|built-in reader|first lines on a page
log|Log file|Text|Card|No|built-in reader|first lines on a page
csv|Comma-separated values|Data|Card|No|built-in reader|first lines on a page
json|JavaScript Object Notation|Data|Card|No|built-in reader|coloured page
jsonc|JSON with comments|Data|Card|No|built-in reader|coloured page
json5|JSON5|Data|Card|No|built-in reader|coloured page
geojson|GeoJSON|Data|Card|No|built-in reader|coloured page
ndjson|Newline-delimited JSON|Data|Card|No|built-in reader|coloured page
jsonl|JSON Lines|Data|Card|No|built-in reader|coloured page
xml|Extensible Markup Language|Data|Card|No|built-in reader|coloured page
xsd|XML Schema|Data|Card|No|built-in reader|coloured page
xaml|Extensible Application Markup Language|Data|Card|No|built-in reader|coloured page
plist|Property list|Data|Card|No|built-in reader|coloured page
yaml|YAML|Data|Card|No|built-in reader|coloured page
yml|YAML|Data|Card|No|built-in reader|coloured page
toml|Tom's Obvious Minimal Language|Data|Card|No|built-in reader|coloured page
ini|INI configuration|Data|Card|No|built-in reader|coloured page
cfg|Configuration file|Data|Card|No|built-in reader|coloured page
conf|Configuration file|Data|Card|No|built-in reader|coloured page
tsv|Tab-separated values|Data|Card|No|built-in reader|coloured page
nfo|Info file (text)|Text|Card|No|built-in reader|coloured page
properties|Java properties|Data|Card|No|built-in reader|coloured page
env|Environment file|Data|Card|No|built-in reader|coloured page
";

/// One row of the table.
#[derive(Clone, Debug)]
pub struct Row {
    pub ext: &'static str,
    pub name: &'static str,
    pub kind: &'static str,
    pub shown: &'static str,
    pub edit: &'static str,
    pub read_by: &'static str,
    pub notes: &'static str,
}

pub fn rows() -> Vec<Row> {
    TABLE
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let f: Vec<&'static str> = l.split('|').collect();
            Row { ext: f[0], name: f[1], kind: f[2], shown: f[3], edit: f[4], read_by: f[5], notes: f.get(6).copied().unwrap_or("") }
        })
        .collect()
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}

fn edit_text(e: &str) -> &'static str {
    match e {
        "Yes" => "Yes - raster picture: edit, save a copy or keep the edits beside it",
        "Flattened" => "Yes, flattened - the merged picture / first frame / first page; layers and animation are not edited",
        "Camera" => "Yes - on the camera's look or the developed picture; the RAW file is never rewritten (edits kept beside it)",
        "Rasterised" => "Yes, rasterised - drawn to pixels first; saving makes a new raster file, the original is untouched",
        _ => "No - nothing to edit (video, sound, 3D, archive, text or a card)",
    }
}

/// The table as CSV (UTF-8 with a byte-order mark so Excel reads it right), sorted by type then extension.
pub fn csv() -> String {
    let mut rows = rows();
    rows.sort_by(|a, b| (a.kind, a.ext).cmp(&(b.kind, b.ext)));
    let mut out = String::from("\u{feff}Extension,Full format name,Type,How NuViewer shows it,Editable in NuEditor,Read by,Notes\r\n");
    for r in rows {
        let shown = match r.shown {
            "View" => "Picture",
            "Animation" => "Animated picture",
            "Play" => "Plays (video / sound) with a poster picture",
            "3D" => "3D view you can turn",
            "Document" => "Pages",
            "Preview" => "The picture stored inside the file",
            _ => "Information card (no pixels)",
        };
        out.push_str(&format!("{},{},{},{},{},{},{}\r\n", csv_field(&format!(".{}", r.ext)), csv_field(r.name), csv_field(r.kind), csv_field(shown), csv_field(edit_text(r.edit)), csv_field(r.read_by), csv_field(r.notes)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn every_extension_the_decoders_know_has_a_row_and_every_row_is_opened() {
        let have: BTreeSet<String> = rows().iter().map(|r| r.ext.to_string()).collect();
        assert_eq!(have.len(), rows().len(), "an extension is listed twice");
        let mut known: BTreeSet<String> = crate::supported_extensions().into_iter().map(String::from).collect();
        for list in [crate::docthumb::ARCHIVE_EXTS, crate::docthumb::OTHER_ARCHIVE_EXTS, crate::docthumb::OFFICE_EXTS, crate::docthumb::OPENDOC_EXTS, crate::docthumb::AFFINITY_EXTS, crate::docthumb::TEXT_EXTS, crate::codepage::CODE_EXTS, &["ecw"][..]] {
            known.extend(list.iter().map(|e| e.to_string()));
        }
        let missing: Vec<&String> = known.iter().filter(|e| !have.contains(*e)).collect();
        assert!(missing.is_empty(), "no row in formats.rs for: {missing:?}");
        let extra: Vec<&String> = have.iter().filter(|e| !known.contains(*e) && !crate::is_supported_ext(e)).collect();
        assert!(extra.is_empty(), "rows for extensions that are not opened: {extra:?}");
        for r in rows() {
            assert!(matches!(r.shown, "View" | "Animation" | "Play" | "3D" | "Document" | "Preview" | "Card"), "{}: {}", r.ext, r.shown);
            assert!(matches!(r.edit, "Yes" | "Flattened" | "Camera" | "Rasterised" | "No"), "{}: {}", r.ext, r.edit);
        }
        assert!(csv().starts_with('\u{feff}'));
    }
}
