//! Unity asset bundles (`.unity3d`, `.unityfs`, `.bundle`, `.ab`, `.assetbundle`): the pictures (Texture2D) inside.
//!
//! A bundle is a `UnityFS` container: a header, a block table (LZ4, LZMA or stored blocks) and a list of files. One of them is a
//! *serialized file* (the objects: textures, sprites, meshes ...), others are the `.resS` / `.resource` streams that hold the big
//! pixel or sound data. The objects are read through their **type tree** (the bundle describes its own fields), so the reader does not
//! depend on the Unity version that wrote it. Only what is needed for pictures is interpreted: the first Texture2D is the picture,
//! the others are its "pages" (`file.unity3d?page=2`, the same way PDF pages are addressed). A bundle without a texture becomes a card
//! that lists what it holds.
//!
//! Nothing here goes round protection: a bundle whose blocks are encrypted or scrambled by a game fails to unpack and says so. Textures: raw
//! formats, BC1-BC7 (`image_dds`), ETC / EAC / ASTC / PVRTC / Crunch (`texture2ddecoder`). Real files checked: BC7, DXT1, RGBA32; the rest only
//! on synthetic blocks. What the reader does not know (serialized files without a type tree, UnityWeb / UnityRaw bundles) is named in the error.

use crate::Image;
use std::path::Path;

pub const EXTS: &[&str] = &["unity3d", "unityfs", "bundle", "ab", "assetbundle"];

pub fn is_ext(ext: &str) -> bool {
    EXTS.contains(&ext.to_ascii_lowercase().as_str())
}

/// Unpacked bundles bigger than this are refused (a damaged size field must not reserve gigabytes).
const MAX_UNPACKED: u64 = 1 << 30;

// ---- reading primitives -----------------------------------------------------------------------------------------------

struct Rd<'a> {
    b: &'a [u8],
    p: usize,
    big: bool,
}

impl<'a> Rd<'a> {
    fn new(b: &'a [u8], big: bool) -> Rd<'a> {
        Rd { b, p: 0, big }
    }
    fn left(&self) -> usize {
        self.b.len().saturating_sub(self.p)
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if n > self.left() {
            return Err("unity: the file ends early".into());
        }
        let s = &self.b[self.p..self.p + n];
        self.p += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn arr<const N: usize>(&mut self) -> Result<[u8; N], String> {
        Ok(self.take(N)?.try_into().unwrap())
    }
    fn u16(&mut self) -> Result<u16, String> {
        let a = self.arr::<2>()?;
        Ok(if self.big { u16::from_be_bytes(a) } else { u16::from_le_bytes(a) })
    }
    fn u32(&mut self) -> Result<u32, String> {
        let a = self.arr::<4>()?;
        Ok(if self.big { u32::from_be_bytes(a) } else { u32::from_le_bytes(a) })
    }
    fn u64(&mut self) -> Result<u64, String> {
        let a = self.arr::<8>()?;
        Ok(if self.big { u64::from_be_bytes(a) } else { u64::from_le_bytes(a) })
    }
    fn i16(&mut self) -> Result<i16, String> {
        Ok(self.u16()? as i16)
    }
    fn i32(&mut self) -> Result<i32, String> {
        Ok(self.u32()? as i32)
    }
    fn i64(&mut self) -> Result<i64, String> {
        Ok(self.u64()? as i64)
    }
    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_bits(self.u32()?))
    }
    fn f64(&mut self) -> Result<f64, String> {
        Ok(f64::from_bits(self.u64()?))
    }
    fn cstr(&mut self) -> Result<String, String> {
        let rest = &self.b[self.p.min(self.b.len())..];
        let n = rest.iter().position(|&c| c == 0).ok_or("unity: a text field has no end")?;
        let s = String::from_utf8_lossy(&rest[..n]).into_owned();
        self.p += n + 1;
        Ok(s)
    }
    fn align(&mut self, n: usize) {
        self.p = (self.p + n - 1) / n * n;
    }
}

// ---- block decompression ----------------------------------------------------------------------------------------------

/// One LZ4 block (no frame): `out_size` bytes come out or it is an error.
fn lz4_block(src: &[u8], out_size: usize) -> Result<Vec<u8>, String> {
    let bad = || "unity: damaged LZ4 block".to_string();
    let mut out: Vec<u8> = Vec::with_capacity(out_size);
    let mut i = 0usize;
    while i < src.len() {
        let token = src[i];
        i += 1;
        let mut lit = (token >> 4) as usize;
        if lit == 15 {
            loop {
                let b = *src.get(i).ok_or_else(bad)?;
                i += 1;
                lit += b as usize;
                if b != 255 {
                    break;
                }
            }
        }
        if i + lit > src.len() || out.len() + lit > out_size {
            return Err(bad());
        }
        out.extend_from_slice(&src[i..i + lit]);
        i += lit;
        if i >= src.len() {
            break;
        }
        if i + 2 > src.len() {
            return Err(bad());
        }
        let off = src[i] as usize | (src[i + 1] as usize) << 8;
        i += 2;
        if off == 0 || off > out.len() {
            return Err(bad());
        }
        let mut m = (token & 15) as usize;
        if m == 15 {
            loop {
                let b = *src.get(i).ok_or_else(bad)?;
                i += 1;
                m += b as usize;
                if b != 255 {
                    break;
                }
            }
        }
        m += 4;
        if out.len() + m > out_size {
            return Err(bad());
        }
        let start = out.len() - off;
        for k in 0..m {
            let v = out[start + k];
            out.push(v);
        }
    }
    if out.len() != out_size {
        return Err(format!("unity: an LZ4 block gave {} bytes, {out_size} were expected", out.len()));
    }
    Ok(out)
}

/// Unity's LZMA: the 5 property bytes (props, dictionary size), then the stream, no length (the block table has it).
fn lzma_block(src: &[u8], out_size: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(out_size);
    let opts = lzma_rs::decompress::Options { unpacked_size: lzma_rs::decompress::UnpackedSize::UseProvided(Some(out_size as u64)), memlimit: None, allow_incomplete: false };
    lzma_rs::lzma_decompress_with_options(&mut std::io::Cursor::new(src), &mut out, &opts).map_err(|e| format!("unity: LZMA block: {e}"))?;
    if out.len() != out_size {
        return Err(format!("unity: an LZMA block gave {} bytes, {out_size} were expected", out.len()));
    }
    Ok(out)
}

fn unpack(kind: u32, src: &[u8], out_size: usize) -> Result<Vec<u8>, String> {
    match kind & 0x3F {
        0 => {
            if src.len() < out_size {
                return Err("unity: a stored block is shorter than it says".into());
            }
            Ok(src[..out_size].to_vec())
        }
        1 => lzma_block(src, out_size),
        2 | 3 => lz4_block(src, out_size),
        n => Err(format!("unity: compression type {n} is not known (the blocks may be scrambled by the game)")),
    }
}

// ---- the bundle -------------------------------------------------------------------------------------------------------

pub struct BundleFile {
    pub name: String,
    /// Bit 2 (4) = a serialized file.
    pub flags: u32,
    pub data: Vec<u8>,
}

pub struct Bundle {
    pub player: String,
    pub engine: String,
    pub files: Vec<BundleFile>,
}

impl Bundle {
    pub fn parse(bytes: &[u8]) -> Result<Bundle, String> {
        let mut r = Rd::new(bytes, true);
        let magic = r.cstr().map_err(|_| "not a Unity bundle".to_string())?;
        match magic.as_str() {
            "UnityFS" => {}
            "UnityWeb" | "UnityRaw" | "UnityArchive" => return Err(format!("{magic} bundles (Unity 3 - 4) are not supported; only UnityFS (Unity 5 and later)")),
            _ => return Err("not a Unity bundle (no UnityFS header)".into()),
        }
        let version = r.u32()?;
        let player = r.cstr()?;
        let engine = r.cstr()?;
        let _size = r.i64()?;
        let comp_size = r.u32()? as usize;
        let raw_size = r.u32()? as usize;
        let flags = r.u32()?;
        if raw_size as u64 > MAX_UNPACKED {
            return Err("unity: the block table claims an absurd size".into());
        }
        if version >= 7 {
            r.align(16);
        }
        let info_packed: &[u8] = if flags & 0x80 != 0 {
            // At the end of the file.
            let start = bytes.len().checked_sub(comp_size).ok_or("unity: the block table lies outside the file")?;
            &bytes[start..]
        } else {
            r.take(comp_size)?
        };
        let info = unpack(flags, info_packed, raw_size)?;
        let mut t = Rd::new(&info, true);
        t.take(16)?; // hash
        let nblocks = t.i32()?;
        if nblocks < 0 || nblocks as usize > info.len() / 10 + 1 {
            return Err("unity: the block table is damaged".into());
        }
        let mut blocks = Vec::new();
        let mut total = 0u64;
        for _ in 0..nblocks {
            let (u, c, f) = (t.u32()? as usize, t.u32()? as usize, t.u16()? as u32);
            total += u as u64;
            blocks.push((u, c, f));
        }
        if total > MAX_UNPACKED {
            return Err(format!("unity: the bundle unpacks to {} MB, which is too much", total >> 20));
        }
        let nnodes = t.i32()?;
        if nnodes < 0 || nnodes as usize > info.len() / 20 + 1 {
            return Err("unity: the file table is damaged".into());
        }
        let mut nodes = Vec::new();
        for _ in 0..nnodes {
            let (o, s, f) = (t.i64()?, t.i64()?, t.u32()?);
            nodes.push((o, s, f, t.cstr()?));
        }
        // The blocks follow the table (or the header) one after the other.
        let mut data = Vec::with_capacity(total as usize);
        for (u, c, f) in blocks {
            let packed = r.take(c)?;
            data.extend_from_slice(&unpack(f, packed, u)?);
        }
        let mut files = Vec::new();
        for (o, s, f, name) in nodes {
            let (o, s) = (o as usize, s as usize);
            if o.checked_add(s).map_or(true, |e| e > data.len()) {
                return Err(format!("unity: the file {name} lies outside the unpacked data"));
            }
            files.push(BundleFile { name, flags: f, data: data[o..o + s].to_vec() });
        }
        Ok(Bundle { player, engine, files })
    }

    /// The stream a texture points to (`archive:/CAB-x/CAB-x.resS` -> the file called `CAB-x.resS`).
    fn stream(&self, path: &str) -> Option<&BundleFile> {
        let leaf = path.rsplit('/').next().unwrap_or(path);
        self.files.iter().find(|f| f.name == leaf).or_else(|| self.files.iter().find(|f| f.name.eq_ignore_ascii_case(leaf)))
    }
}

// ---- serialized files -------------------------------------------------------------------------------------------------

/// Unity's built-in string table for type trees (a name with the high bit set is an offset in here).
const COMMON_STRINGS: &str = "AABB\0AnimationClip\0AnimationCurve\0AnimationState\0Array\0Base\0BitField\0bitset\0bool\0char\0ColorRGBA\0Component\0data\0deque\0double\0dynamic_array\0FastPropertyName\0first\0float\0Font\0GameObject\0Generic Mono\0GradientNEW\0GUID\0GUIStyle\0int\0list\0long long\0map\0Matrix4x4f\0MdFour\0MonoBehaviour\0MonoScript\0m_ByteSize\0m_Curve\0m_EditorClassIdentifier\0m_EditorHideFlags\0m_Enabled\0m_ExtensionPtr\0m_GameObject\0m_Index\0m_IsArray\0m_IsStatic\0m_MetaFlag\0m_Name\0m_ObjectHideFlags\0m_PrefabInternal\0m_PrefabParentObject\0m_Script\0m_StaticEditorFlags\0m_Type\0m_Version\0Object\0pair\0PPtr<Component>\0PPtr<GameObject>\0PPtr<Material>\0PPtr<MonoBehaviour>\0PPtr<MonoScript>\0PPtr<Object>\0PPtr<Prefab>\0PPtr<Sprite>\0PPtr<TextAsset>\0PPtr<Texture>\0PPtr<Texture2D>\0PPtr<Transform>\0Prefab\0Quaternionf\0Rectf\0RectInt\0RectOffset\0second\0set\0short\0size\0SInt16\0SInt32\0SInt64\0SInt8\0staticvector\0string\0TextAsset\0TextMesh\0Texture\0Texture2D\0Transform\0TypelessData\0UInt16\0UInt32\0UInt64\0UInt8\0unsigned int\0unsigned long long\0unsigned short\0vector\0Vector2f\0Vector3f\0Vector4f\0m_ScriptingClassIdentifier\0Gradient\0Type*\0int2_storage\0int3_storage\0BoundsInt\0m_CorrespondingSourceObject\0m_PrefabInstance\0m_PrefabAsset\0FileSize\0Hash128\0";

fn string_at(table: &[u8], off: u32) -> String {
    let (t, o) = if off & 0x8000_0000 != 0 { (COMMON_STRINGS.as_bytes(), (off & 0x7FFF_FFFF) as usize) } else { (table, off as usize) };
    let Some(rest) = t.get(o..) else { return String::new() };
    let n = rest.iter().position(|&c| c == 0).unwrap_or(rest.len());
    String::from_utf8_lossy(&rest[..n]).into_owned()
}

#[derive(Clone, Debug)]
struct Node {
    level: u8,
    is_array: bool,
    type_name: String,
    name: String,
    meta: i32,
}

struct TypeInfo {
    class_id: i32,
    nodes: Option<Vec<Node>>,
}

pub struct Object {
    pub path_id: i64,
    pub class_id: i32,
    type_index: usize,
    start: usize,
    size: usize,
}

pub struct Serialized<'a> {
    data: &'a [u8],
    pub version: u32,
    pub unity: String,
    little: bool,
    types: Vec<TypeInfo>,
    pub objects: Vec<Object>,
    data_offset: usize,
}

impl<'a> Serialized<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Serialized<'a>, String> {
        let mut r = Rd::new(data, true);
        let (mut meta_size, mut file_size, version, mut data_offset) = (r.u32()? as u64, r.u32()? as u64, r.u32()?, r.u32()? as u64);
        let _ = (&mut meta_size, &mut file_size);
        if !(14..=22).contains(&version) {
            return Err(format!("unity: serialized file version {version} is not supported (Unity 2017 - 2022 write 14 - 22)"));
        }
        let little = r.u8()? == 0;
        r.take(3)?;
        if version >= 22 {
            meta_size = r.u32()? as u64;
            file_size = r.i64()? as u64;
            data_offset = r.i64()? as u64;
            r.take(8)?;
        }
        let _ = (meta_size, file_size);
        r.big = !little;
        let unity = r.cstr()?;
        let _platform = r.i32()?;
        let type_tree = r.u8()? != 0;
        let ntypes = r.i32()?;
        if ntypes < 0 || ntypes as usize > data.len() {
            return Err("unity: the type table is damaged".into());
        }
        let mut types = Vec::new();
        for _ in 0..ntypes {
            types.push(read_type(&mut r, version, type_tree, false)?);
        }
        let nobj = r.i32()?;
        if nobj < 0 || nobj as usize > data.len() / 8 + 1 {
            return Err("unity: the object table is damaged".into());
        }
        let mut objects = Vec::new();
        for _ in 0..nobj {
            r.align(4);
            let path_id = r.i64()?;
            let start = if version >= 22 { r.i64()? as usize } else { r.u32()? as usize };
            let size = r.u32()? as usize;
            let type_index = r.i32()?;
            let ty = types.get(type_index as usize).ok_or("unity: an object has a type that does not exist")?;
            if (11..17).contains(&version) {
                r.i16()?; // script type index
            }
            if version == 15 || version == 16 {
                r.u8()?; // stripped
            }
            objects.push(Object { path_id, class_id: ty.class_id, type_index: type_index as usize, start, size });
        }
        Ok(Serialized { data, version, unity, little, types, objects, data_offset: data_offset as usize })
    }

    /// The bytes of an object.
    fn bytes_of(&self, o: &Object) -> Result<&'a [u8], String> {
        let s = self.data_offset.checked_add(o.start).ok_or("unity: object outside the file")?;
        let e = s.checked_add(o.size).ok_or("unity: object outside the file")?;
        self.data.get(s..e).ok_or_else(|| "unity: an object lies outside the file".to_string())
    }

    /// The fields of an object, read through its type tree.
    pub fn read(&self, o: &Object) -> Result<Val, String> {
        let nodes = self.types[o.type_index].nodes.as_ref().ok_or("unity: this serialized file has no type tree (stripped), so its objects can't be read")?;
        let mut r = Rd::new(self.bytes_of(o)?, !self.little);
        let (v, _) = read_value(nodes, 0, &mut r, 0)?;
        Ok(v)
    }
}

fn read_type(r: &mut Rd, version: u32, type_tree: bool, is_ref: bool) -> Result<TypeInfo, String> {
    let class_id = r.i32()?;
    if version >= 16 {
        r.u8()?; // stripped
    }
    let mut script_index = -1i16;
    if version >= 17 {
        script_index = r.i16()?;
    }
    if (is_ref && script_index >= 0) || (version >= 16 && class_id == 114) {
        r.take(16)?; // script hash
    }
    r.take(16)?; // type hash
    let mut nodes = None;
    if type_tree {
        let n = r.i32()?;
        let strings = r.i32()?;
        if n < 0 || strings < 0 || n as usize > r.left() / 24 + 1 {
            return Err("unity: a type tree is damaged".into());
        }
        let mut raw = Vec::new();
        for _ in 0..n {
            r.u16()?; // version
            let level = r.u8()?;
            let is_array = r.u8()? != 0;
            let (t, nm) = (r.u32()?, r.u32()?);
            r.i32()?; // byte size
            r.i32()?; // index
            let meta = r.i32()?;
            if version >= 19 {
                r.u64()?; // ref type hash
            }
            raw.push((level, is_array, t, nm, meta));
        }
        let table = r.take(strings as usize)?;
        nodes = Some(raw.into_iter().map(|(level, is_array, t, nm, meta)| Node { level, is_array, type_name: string_at(table, t), name: string_at(table, nm), meta }).collect());
        if version >= 21 {
            if is_ref {
                r.cstr()?;
                r.cstr()?;
                r.cstr()?;
            } else {
                let n = r.i32()?;
                if n < 0 || n as usize * 4 > r.left() {
                    return Err("unity: a type tree is damaged".into());
                }
                r.take(n as usize * 4)?;
            }
        }
    }
    Ok(TypeInfo { class_id, nodes })
}

// ---- values read through a type tree ---------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum Val {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Bytes(Vec<u8>),
    List(Vec<Val>),
    Obj(Vec<(String, Val)>),
}

impl Val {
    pub fn get(&self, key: &str) -> Option<&Val> {
        match self {
            Val::Obj(f) => f.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn int(&self) -> Option<i64> {
        match self {
            Val::Int(i) => Some(*i),
            Val::Bool(b) => Some(*b as i64),
            _ => None,
        }
    }
    pub fn text(&self) -> Option<&str> {
        match self {
            Val::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn bytes(&self) -> Option<&[u8]> {
        match self {
            Val::Bytes(b) => Some(b),
            _ => None,
        }
    }
}

/// Read the value of node `i`; returns it and the index after the node's subtree.
fn read_value(nodes: &[Node], i: usize, r: &mut Rd, depth: usize) -> Result<(Val, usize), String> {
    if depth > 40 {
        return Err("unity: a type tree nests too deeply".into());
    }
    let node = &nodes[i];
    let mut end = i + 1;
    while end < nodes.len() && nodes[end].level > node.level {
        end += 1;
    }
    let align = node.meta & 0x4000 != 0;
    let val = match node.type_name.as_str() {
        "SInt8" => Val::Int(r.u8()? as i8 as i64),
        "UInt8" | "char" => Val::Int(r.u8()? as i64),
        "bool" => Val::Bool(r.u8()? != 0),
        "SInt16" | "short" => Val::Int(r.i16()? as i64),
        "UInt16" | "unsigned short" => Val::Int(r.u16()? as i64),
        "SInt32" | "int" => Val::Int(r.i32()? as i64),
        "UInt32" | "unsigned int" | "Type*" => Val::Int(r.u32()? as i64),
        "SInt64" | "long long" | "UInt64" | "unsigned long long" | "FileSize" => Val::Int(r.i64()?),
        "float" => Val::Float(r.f32()? as f64),
        "double" => Val::Float(r.f64()?),
        "string" => {
            let n = r.i32()?;
            if n < 0 || n as usize > r.left() {
                return Err("unity: a string is longer than the object".into());
            }
            let s = String::from_utf8_lossy(r.take(n as usize)?).into_owned();
            r.align(4);
            Val::Str(s)
        }
        "TypelessData" => {
            let n = r.i32()?;
            if n < 0 || n as usize > r.left() {
                return Err("unity: a data block is longer than the object".into());
            }
            Val::Bytes(r.take(n as usize)?.to_vec())
        }
        _ if node.is_array => {
            // `Array` node: size, then `data` elements (children: size, data).
            let n = r.i32()?;
            let data_node = i + 2;
            if n < 0 || data_node >= nodes.len() || n as usize > r.left().saturating_add(16) {
                return Err("unity: an array is longer than the object".into());
            }
            let n = n as usize;
            if matches!(nodes[data_node].type_name.as_str(), "UInt8" | "SInt8" | "char") {
                Val::Bytes(r.take(n)?.to_vec())
            } else {
                let mut v = Vec::with_capacity(n.min(4096));
                for _ in 0..n {
                    v.push(read_value(nodes, data_node, r, depth + 1)?.0);
                }
                Val::List(v)
            }
        }
        _ => {
            let mut fields = Vec::new();
            let mut k = i + 1;
            while k < end {
                let (v, next) = read_value(nodes, k, r, depth + 1)?;
                fields.push((nodes[k].name.clone(), v));
                k = next;
            }
            // `vector` / `map` / `set`: the Array child stands for the node.
            if fields.len() == 1 && nodes[i + 1].is_array {
                fields.pop().unwrap().1
            } else {
                Val::Obj(fields)
            }
        }
    };
    if align {
        r.align(4);
    }
    Ok((val, end))
}

// ---- textures ---------------------------------------------------------------------------------------------------------

pub struct Texture {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub format: i64,
    /// Top row first.
    pub rgba: Vec<u8>,
}

pub fn format_name(f: i64) -> String {
    match f {
        1 => "Alpha8",
        2 => "ARGB4444",
        3 => "RGB24",
        4 => "RGBA32",
        5 => "ARGB32",
        7 => "RGB565",
        9 => "R16",
        10 => "DXT1",
        12 => "DXT5",
        13 => "RGBA4444",
        14 => "BGRA32",
        15 => "RHalf",
        16 => "RGHalf",
        17 => "RGBAHalf",
        18 => "RFloat",
        19 => "RGFloat",
        20 => "RGBAFloat",
        24 => "BC6H",
        25 => "BC7",
        26 => "BC4",
        27 => "BC5",
        28 => "DXT1 Crunched",
        29 => "DXT5 Crunched",
        30..=33 => "PVRTC",
        34 => "ETC RGB4",
        41..=44 => "EAC",
        45 => "ETC2 RGB",
        46 => "ETC2 RGBA1",
        47 => "ETC2 RGBA8",
        48..=59 => "ASTC",
        62 => "RG16",
        63 => "R8",
        64 | 65 => "ETC Crunched",
        n => return format!("format {n}"),
    }
    .to_string()
}

/// Bytes of the top mip of a `w` x `h` texture in `format`, or None for formats this reader does not decode.
fn level0_size(format: i64, w: usize, h: usize) -> Option<usize> {
    let blocks = |bw: usize, bh: usize, bytes: usize| w.div_ceil(bw) * h.div_ceil(bh) * bytes;
    Some(match format {
        1 | 63 => w * h,
        2 | 7 | 9 | 13 | 62 => w * h * 2,
        3 => w * h * 3,
        4 | 5 | 14 => w * h * 4,
        10 | 26 | 34 | 41 | 42 | 45 | 46 => blocks(4, 4, 8),
        12 | 24 | 25 | 27 | 43 | 44 | 47 => blocks(4, 4, 16),
        48..=59 => {
            let (bw, bh) = astc_block(format);
            blocks(bw, bh, 16)
        }
        30 | 31 => w.max(16) * h.max(8) / 4,
        32 | 33 => w.max(8) * h.max(8) / 2,
        _ => return None,
    })
}

/// ASTC block footprint of a Unity texture format (48 - 53 RGB, 54 - 59 RGBA: 4x4, 5x5, 6x6, 8x8, 10x10, 12x12).
fn astc_block(format: i64) -> (usize, usize) {
    let n = [4usize, 5, 6, 8, 10, 12][((format - 48) % 6) as usize];
    (n, n)
}

/// Formats read by the `texture2ddecoder` crate (ETC, EAC, ASTC, PVRTC and Crunch): its pixels are packed B, G, R, A.
fn decode_other(format: i64, w: usize, h: usize, d: &[u8]) -> Result<Vec<u8>, String> {
    use texture2ddecoder as t;
    let mut px = vec![0u32; w * h];
    let r = match format {
        34 => t::decode_etc1(d, w, h, &mut px),
        45 => t::decode_etc2_rgb(d, w, h, &mut px),
        46 => t::decode_etc2_rgba1(d, w, h, &mut px),
        47 => t::decode_etc2_rgba8(d, w, h, &mut px),
        41 => t::decode_eacr(d, w, h, &mut px),
        42 => t::decode_eacr_signed(d, w, h, &mut px),
        43 => t::decode_eacrg(d, w, h, &mut px),
        44 => t::decode_eacrg_signed(d, w, h, &mut px),
        48..=59 => {
            let (bw, bh) = astc_block(format);
            t::decode_astc(d, w, h, bw, bh, &mut px)
        }
        30 | 31 => t::decode_pvrtc_2bpp(d, w, h, &mut px),
        32 | 33 => t::decode_pvrtc_4bpp(d, w, h, &mut px),
        28 | 29 | 64 | 65 => t::decode_unity_crunch(d, w, h, &mut px),
        _ => return Err(format!("unity: {} textures are not supported yet", format_name(format))),
    };
    r.map_err(|e| format!("unity: {} texture: {e}", format_name(format)))?;
    let mut out = Vec::with_capacity(w * h * 4);
    for v in px {
        let [b, g, r, a] = v.to_le_bytes();
        out.extend_from_slice(&[r, g, b, a]);
    }
    // The RGB-only formats carry no alpha (the crate fills 255 for them, EAC gives its own channels).
    if matches!(format, 41 | 42) {
        for o in out.chunks_exact_mut(4) {
            let g = o[0];
            o[1] = g;
            o[2] = g;
            o[3] = 255;
        }
    }
    Ok(out)
}

fn decode_pixels(format: i64, w: usize, h: usize, d: &[u8]) -> Result<Vec<u8>, String> {
    if matches!(format, 28 | 29 | 64 | 65) {
        return decode_other(format, w, h, d); // Crunch: the stream carries its own size
    }
    let need = level0_size(format, w, h).ok_or_else(|| format!("unity: {} textures are not supported yet", format_name(format)))?;
    if !matches!(format, 1 | 2 | 3 | 4 | 5 | 7 | 9 | 10 | 12 | 13 | 14 | 24..=27 | 62 | 63) {
        if d.len() < need {
            return Err(format!("unity: the texture has {} bytes of pixels, {need} were expected", d.len()));
        }
        return decode_other(format, w, h, &d[..need]);
    }
    if d.len() < need {
        return Err(format!("unity: the texture has {} bytes of pixels, {need} were expected", d.len()));
    }
    let d = &d[..need];
    let mut out = vec![255u8; w * h * 4];
    let nib = |v: u16, shift: u32| (((v >> shift) & 15) as u8) * 17;
    match format {
        1 => {
            for (o, &a) in out.chunks_exact_mut(4).zip(d) {
                o[3] = a;
            }
        }
        63 => {
            for (o, &g) in out.chunks_exact_mut(4).zip(d) {
                o[..3].fill(g);
            }
        }
        9 => {
            for (o, p) in out.chunks_exact_mut(4).zip(d.chunks_exact(2)) {
                o[..3].fill(p[1]); // the high byte of the 16-bit grey
            }
        }
        62 => {
            for (o, p) in out.chunks_exact_mut(4).zip(d.chunks_exact(2)) {
                o[0] = p[0];
                o[1] = p[1];
                o[2] = 0;
            }
        }
        2 => {
            for (o, p) in out.chunks_exact_mut(4).zip(d.chunks_exact(2)) {
                let v = u16::from_le_bytes([p[0], p[1]]);
                o.copy_from_slice(&[nib(v, 8), nib(v, 4), nib(v, 0), nib(v, 12)]);
            }
        }
        13 => {
            for (o, p) in out.chunks_exact_mut(4).zip(d.chunks_exact(2)) {
                let v = u16::from_le_bytes([p[0], p[1]]);
                o.copy_from_slice(&[nib(v, 12), nib(v, 8), nib(v, 4), nib(v, 0)]);
            }
        }
        7 => {
            for (o, p) in out.chunks_exact_mut(4).zip(d.chunks_exact(2)) {
                let v = u16::from_le_bytes([p[0], p[1]]) as u32;
                let (r5, g6, b5) = ((v >> 11) & 31, (v >> 5) & 63, v & 31);
                o[0] = ((r5 << 3) | (r5 >> 2)) as u8;
                o[1] = ((g6 << 2) | (g6 >> 4)) as u8;
                o[2] = ((b5 << 3) | (b5 >> 2)) as u8;
            }
        }
        3 => {
            for (o, p) in out.chunks_exact_mut(4).zip(d.chunks_exact(3)) {
                o[..3].copy_from_slice(p);
            }
        }
        4 => out.copy_from_slice(d),
        5 => {
            for (o, p) in out.chunks_exact_mut(4).zip(d.chunks_exact(4)) {
                o.copy_from_slice(&[p[1], p[2], p[3], p[0]]);
            }
        }
        14 => {
            for (o, p) in out.chunks_exact_mut(4).zip(d.chunks_exact(4)) {
                o.copy_from_slice(&[p[2], p[1], p[0], p[3]]);
            }
        }
        10 | 12 | 24 | 25 | 26 | 27 => {
            use image_dds::{ImageFormat, Surface};
            let f = match format {
                10 => ImageFormat::BC1RgbaUnorm,
                12 => ImageFormat::BC3RgbaUnorm,
                24 => ImageFormat::BC6hRgbUfloat,
                25 => ImageFormat::BC7RgbaUnorm,
                26 => ImageFormat::BC4RUnorm,
                _ => ImageFormat::BC5RgUnorm,
            };
            let s = Surface { width: w as u32, height: h as u32, depth: 1, layers: 1, mipmaps: 1, image_format: f, data: d };
            let dec = s.decode_rgba8().map_err(|e| format!("unity: {} block decoding: {e}", format_name(format)))?;
            out = dec.data;
            if format == 26 {
                for o in out.chunks_exact_mut(4) {
                    let g = o[0];
                    o[1] = g;
                    o[2] = g;
                    o[3] = 255;
                }
            }
        }
        _ => unreachable!(),
    }
    Ok(out)
}

/// A Texture2D object -> pixels (top row first).
fn texture_of(v: &Val, bundle: &Bundle) -> Result<Texture, String> {
    let int = |k: &str| v.get(k).and_then(Val::int).ok_or_else(|| format!("unity: the texture has no {k}"));
    let (w, h) = (int("m_Width")?, int("m_Height")?);
    let format = int("m_TextureFormat")?;
    if !(1..=16384).contains(&w) || !(1..=16384).contains(&h) {
        return Err(format!("unity: a texture of {w}x{h} pixels"));
    }
    crate::check_size(w as u64, h as u64)?;
    if v.get("m_TextureDimension").and_then(Val::int).is_some_and(|d| d != 2) {
        return Err("unity: only flat (2D) textures are shown, not cube maps or volumes".into());
    }
    let name = v.get("m_Name").and_then(Val::text).unwrap_or("").to_string();
    let inline = v.get("image data").and_then(Val::bytes).unwrap_or(&[]);
    let data: &[u8] = if !inline.is_empty() {
        inline
    } else {
        let sd = v.get("m_StreamData").ok_or("unity: the texture has no pixels")?;
        let (off, size) = (sd.get("offset").and_then(Val::int).unwrap_or(0) as usize, sd.get("size").and_then(Val::int).unwrap_or(0) as usize);
        let path = sd.get("path").and_then(Val::text).unwrap_or("");
        let f = bundle.stream(path).ok_or_else(|| format!("unity: the pixel stream {path} is not in the bundle"))?;
        f.data.get(off..off.saturating_add(size).min(f.data.len())).ok_or("unity: the pixel stream is shorter than the texture says")?
    };
    let mut rgba = decode_pixels(format, w as usize, h as usize, data)?;
    // Unity stores the bottom row first.
    let row = w as usize * 4;
    for y in 0..h as usize / 2 {
        let (a, b) = rgba.split_at_mut((h as usize - 1 - y) * row);
        a[y * row..(y + 1) * row].swap_with_slice(&mut b[..row]);
    }
    Ok(Texture { name, width: w as u32, height: h as u32, format, rgba })
}

// ---- what is in a bundle ----------------------------------------------------------------------------------------------

pub fn class_name(id: i32) -> &'static str {
    match id {
        1 => "GameObject",
        4 => "Transform",
        21 => "Material",
        23 => "MeshRenderer",
        28 => "Texture2D",
        33 => "MeshFilter",
        43 => "Mesh",
        48 => "Shader",
        49 => "TextAsset",
        74 => "AnimationClip",
        82 => "AudioSource",
        83 => "AudioClip",
        90 => "Avatar",
        91 => "AnimatorController",
        95 => "Animator",
        114 => "MonoBehaviour",
        115 => "MonoScript",
        128 => "Font",
        142 => "AssetBundle",
        150 => "PreloadData",
        152 => "MovieTexture",
        184 => "Sprite (old)",
        212 => "SpriteRenderer",
        213 => "Sprite",
        221 => "AnimatorOverrideController",
        687078895 => "SpriteAtlas",
        _ => "other",
    }
}

pub struct Contents {
    pub bundle: Bundle,
    /// (file index in the bundle, object index) of every Texture2D.
    pub textures: Vec<(usize, usize)>,
    /// (class id, count) of all objects.
    pub classes: Vec<(i32, usize)>,
    pub objects: usize,
}

fn is_stream_name(n: &str) -> bool {
    n.ends_with(".resS") || n.ends_with(".resource") || n.ends_with(".ress")
}

pub fn open(bytes: &[u8]) -> Result<Contents, String> {
    let bundle = Bundle::parse(bytes)?;
    let mut textures = Vec::new();
    let mut counts: std::collections::BTreeMap<i32, usize> = Default::default();
    let mut objects = 0;
    let mut first_err = None;
    let mut any = false;
    for (fi, f) in bundle.files.iter().enumerate() {
        if is_stream_name(&f.name) || f.data.len() < 32 {
            continue;
        }
        match Serialized::parse(&f.data) {
            Ok(s) => {
                any = true;
                for (oi, o) in s.objects.iter().enumerate() {
                    *counts.entry(o.class_id).or_default() += 1;
                    objects += 1;
                    if o.class_id == 28 {
                        textures.push((fi, oi));
                    }
                }
            }
            Err(e) => {
                first_err.get_or_insert(e);
            }
        }
    }
    if !any {
        return Err(first_err.unwrap_or_else(|| "unity: the bundle holds no serialized file".into()));
    }
    let mut classes: Vec<(i32, usize)> = counts.into_iter().collect();
    classes.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    Ok(Contents { bundle, textures, classes, objects })
}

impl Contents {
    /// Decode texture number `n` (0-based).
    pub fn texture(&self, n: usize) -> Result<Texture, String> {
        let &(fi, oi) = self.textures.get(n).ok_or("unity: no such texture")?;
        let f = &self.bundle.files[fi];
        let s = Serialized::parse(&f.data)?;
        let v = s.read(&s.objects[oi])?;
        texture_of(&v, &self.bundle)
    }

    /// Names of the textures without decoding any (for lists and cards).
    pub fn texture_names(&self) -> Vec<String> {
        self.textures
            .iter()
            .map(|&(fi, oi)| {
                let f = &self.bundle.files[fi];
                Serialized::parse(&f.data).ok().and_then(|s| s.read(&s.objects[oi]).ok()).and_then(|v| v.get("m_Name").and_then(Val::text).map(str::to_string)).unwrap_or_default()
            })
            .collect()
    }

    fn card_lines(&self) -> Vec<String> {
        let mut lines = vec![format!("Unity {}", self.bundle.engine), format!("{} files, {} objects", self.bundle.files.len(), self.objects)];
        for (id, n) in self.classes.iter().take(8) {
            lines.push(format!("{n} x {}", class_name(*id)));
        }
        lines
    }
}

/// The picture of a bundle: texture `page` (1-based; the first by default) as an image, or a card when it holds no texture.
pub fn decode(path: &Path, page: u32, first_only: bool) -> Result<Image, String> {
    let bytes = crate::progress::read_file(path)?;
    let c = open(&bytes)?;
    if c.textures.is_empty() {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("unity3d").to_ascii_lowercase();
        let mut img = crate::docthumb::render_card(
            &crate::docthumb::Card { badge: "UNITY".into(), accent: crate::docthumb::accent_for(&ext), title: name, subtitle: crate::docthumb::fmt_bytes(bytes.len() as u64), lines: Vec::new() },
            if first_only { 1 } else { 2 },
        );
        img.colour_note = Some(format!("a card: no texture in this bundle ({})", c.card_lines().join(", ")));
        return Ok(img);
    }
    let n = (page.max(1) as usize - 1).min(c.textures.len() - 1);
    let t = c.texture(n)?;
    let mut img = Image::new(t.width, t.height, t.rgba);
    img.colour_note = Some(format!("Unity {} texture {}{}: {}x{} {}", c.bundle.engine, n + 1, if t.name.is_empty() { String::new() } else { format!(" \"{}\"", t.name) }, t.width, t.height, format_name(t.format)));
    if c.textures.len() > 1 {
        img.page = Some((n as u32 + 1, c.textures.len() as u32));
    }
    crate::check_buffers(img)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lz4_literals_matches_and_overlapping_copies() {
        // "abcabcabcabc": 3 literals, then a match of 9 bytes at offset 3 (overlapping).
        let src = [0x35u8, b'a', b'b', b'c', 0x03, 0x00];
        assert_eq!(lz4_block(&src, 12).unwrap(), b"abcabcabcabc");
        assert!(lz4_block(&src, 13).is_err(), "wrong size is an error");
        assert!(lz4_block(&[0x10, b'a', 0x09, 0x00], 6).is_err(), "offset beyond the output");
    }

    #[test]
    fn common_string_offsets_match_unitys_table() {
        let at = |s: &str| COMMON_STRINGS.find(&format!("{s}\0")).unwrap() as u32;
        assert_eq!(at("string"), 840);
        assert_eq!(at("m_Name"), 427);
        assert_eq!(at("size"), 795);
        assert_eq!(string_at(b"", 0x8000_0000 | 840), "string");
        assert_eq!(string_at(b"local\0", 0), "local");
    }

    #[test]
    fn raw_formats_become_rgba() {
        // RGB565 red, 2 x 1.
        let px = decode_pixels(7, 2, 1, &[0x00, 0xF8, 0x1F, 0x00]).unwrap();
        assert_eq!(&px[..4], &[255, 0, 0, 255]);
        assert_eq!(&px[4..], &[0, 0, 255, 255]);
        // ARGB32 bytes A R G B.
        assert_eq!(decode_pixels(5, 1, 1, &[10, 1, 2, 3]).unwrap(), vec![1, 2, 3, 10]);
        assert_eq!(decode_pixels(14, 1, 1, &[3, 2, 1, 9]).unwrap(), vec![1, 2, 3, 9]);
        assert!(decode_pixels(99, 4, 4, &[0; 16]).unwrap_err().contains("not supported"));
        assert!(decode_pixels(4, 2, 2, &[0; 8]).is_err(), "too few bytes");
    }

    #[test]
    fn compressed_formats_go_through_the_block_decoders() {
        // ASTC void-extent block (a single colour): opaque red, 4x4 and 6x6 footprints.
        let mut astc = vec![0xFC, 0xFD, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        astc.extend([0xFF, 0xFF, 0, 0, 0, 0, 0xFF, 0xFF]);
        let px = decode_pixels(48, 4, 4, &astc).unwrap();
        assert!(px.chunks_exact(4).all(|p| p == [255, 0, 0, 255]), "{:?}", &px[..8]);
        let px = decode_pixels(50, 6, 6, &astc).unwrap();
        assert_eq!(px.len(), 6 * 6 * 4);
        assert!(px.chunks_exact(4).all(|p| p == [255, 0, 0, 255]));
        // ETC2 RGB block of zeros decodes without complaint; sizes are checked.
        assert_eq!(decode_pixels(45, 8, 4, &[0; 16]).unwrap().len(), 8 * 4 * 4);
        assert!(decode_pixels(45, 8, 4, &[0; 8]).is_err());
        assert_eq!(level0_size(47, 5, 5), Some(4 * 16));
        assert_eq!(level0_size(54, 12, 12), Some(9 * 16));
        assert_eq!(level0_size(55, 10, 10), Some(4 * 16), "5x5 blocks");
    }

    #[test]
    fn not_a_bundle_says_so() {
        assert!(Bundle::parse(b"hello world").is_err());
        assert!(Bundle::parse(b"UnityWeb\0\0\0\0\x03").err().unwrap().contains("not supported"));
    }

    /// A bundle built here: stored blocks, one serialized file (version 21, type tree, two Texture2D objects with inline RGBA32 pixels).
    fn synthetic_bundle() -> Vec<u8> {
        fn put_str(v: &mut Vec<u8>, s: &str) {
            v.extend((s.len() as i32).to_le_bytes());
            v.extend(s.as_bytes());
            while v.len() % 4 != 0 {
                v.push(0);
            }
        }
        // The type tree: Texture2D { string m_Name; int m_Width, m_Height, m_TextureFormat, m_TextureDimension; TypelessData "image data" }.
        let names = ["Texture2D", "Base", "string", "m_Name", "int", "m_Width", "m_Height", "m_TextureFormat", "m_TextureDimension", "TypelessData", "image data"];
        let mut table = Vec::new();
        let mut off = std::collections::HashMap::new();
        for n in names {
            off.insert(n, table.len() as u32);
            table.extend(n.as_bytes());
            table.push(0);
        }
        let node = |level: u8, ty: &str, name: &str, meta: i32| {
            let mut n = Vec::new();
            n.extend(1u16.to_le_bytes());
            n.push(level);
            n.push(0);
            n.extend(off[ty].to_le_bytes());
            n.extend(off[name].to_le_bytes());
            n.extend((-1i32).to_le_bytes());
            n.extend(0i32.to_le_bytes());
            n.extend(meta.to_le_bytes());
            n.extend(0u64.to_le_bytes());
            n
        };
        let nodes = [node(0, "Texture2D", "Base", 0), node(1, "string", "m_Name", 0x4000), node(1, "int", "m_Width", 0), node(1, "int", "m_Height", 0), node(1, "int", "m_TextureFormat", 0), node(1, "int", "m_TextureDimension", 0), node(1, "TypelessData", "image data", 0x4000)];
        let texture = |name: &str, w: i32, h: i32, px: &[u8]| {
            let mut v = Vec::new();
            put_str(&mut v, name);
            for x in [w, h, 4, 2] {
                v.extend(x.to_le_bytes());
            }
            v.extend((px.len() as i32).to_le_bytes());
            v.extend(px);
            while v.len() % 4 != 0 {
                v.push(0);
            }
            v
        };
        // 2 x 2, bottom row first: red, red / blue, blue.
        let red = [255u8, 0, 0, 255];
        let blue = [0u8, 0, 255, 255];
        let a = texture("first", 2, 2, &[red, red, blue, blue].concat());
        let b = texture("second", 1, 1, &[0, 255, 0, 128]);
        // Metadata.
        let mut m = Vec::new();
        m.extend(b"2019.4.0f1\0");
        m.extend(19i32.to_le_bytes());
        m.push(1);
        m.extend(1i32.to_le_bytes()); // one type
        m.extend(28i32.to_le_bytes());
        m.push(0);
        m.extend((-1i16).to_le_bytes());
        m.extend([0u8; 16]);
        m.extend((nodes.len() as i32).to_le_bytes());
        m.extend((table.len() as i32).to_le_bytes());
        for n in &nodes {
            m.extend(n);
        }
        m.extend(&table);
        m.extend(0i32.to_le_bytes()); // type dependencies
        m.extend(2i32.to_le_bytes()); // two objects
        let header_len = 20; // 4 x u32, the endian byte and 3 reserved bytes: the file offset of the metadata
        let mut objs = Vec::new();
        let mut cur = header_len + m.len();
        let mut start = 0u32;
        let mut entries = Vec::new();
        for (id, data) in [(1i64, &a), (2, &b)] {
            while (cur + objs.len()) % 4 != 0 {
                objs.push(0);
            }
            objs.extend(id.to_le_bytes());
            objs.extend(start.to_le_bytes());
            objs.extend((data.len() as u32).to_le_bytes());
            objs.extend(0i32.to_le_bytes());
            entries.push(data);
            start += data.len() as u32;
        }
        let _ = &mut cur;
        let mut meta_part = m;
        meta_part.extend(&objs);
        let data_offset = (header_len + meta_part.len() + 15) / 16 * 16;
        let mut file = Vec::new();
        file.extend(((header_len - 20) as u32 + (meta_part.len() as u32)).to_be_bytes()); // metadata size (not used by the reader)
        file.extend(0u32.to_be_bytes());
        file.extend(21u32.to_be_bytes());
        file.extend((data_offset as u32).to_be_bytes());
        file.push(0); // little endian
        file.extend([0u8; 3]);
        assert_eq!(file.len(), header_len);
        file.extend(&meta_part);
        file.resize(data_offset, 0);
        for e in entries {
            file.extend(e);
        }
        // The bundle around it.
        let mut info = Vec::new();
        info.extend([0u8; 16]);
        info.extend(1i32.to_be_bytes());
        info.extend((file.len() as u32).to_be_bytes());
        info.extend((file.len() as u32).to_be_bytes());
        info.extend(0u16.to_be_bytes());
        info.extend(1i32.to_be_bytes());
        info.extend(0i64.to_be_bytes());
        info.extend((file.len() as i64).to_be_bytes());
        info.extend(4u32.to_be_bytes());
        info.extend(b"CAB-test\0");
        let mut b = Vec::new();
        b.extend(b"UnityFS\0");
        b.extend(7u32.to_be_bytes());
        b.extend(b"5.x.x\0");
        b.extend(b"2019.4.0f1\0");
        b.extend(0i64.to_be_bytes());
        b.extend((info.len() as u32).to_be_bytes());
        b.extend((info.len() as u32).to_be_bytes());
        b.extend(0u32.to_be_bytes());
        while b.len() % 16 != 0 {
            b.push(0);
        }
        b.extend(&info);
        b.extend(&file);
        b
    }

    #[test]
    fn a_built_bundle_with_two_textures_gives_two_pages_upright() {
        let bytes = synthetic_bundle();
        let c = open(&bytes).unwrap();
        assert_eq!(c.textures.len(), 2);
        assert_eq!(c.texture_names(), vec!["first".to_string(), "second".to_string()]);
        let t = c.texture(0).unwrap();
        assert_eq!((t.width, t.height, t.format), (2, 2, 4));
        // Unity keeps the bottom row first: after decoding the top row is the blue one.
        assert_eq!(&t.rgba[..4], &[0, 0, 255, 255]);
        assert_eq!(&t.rgba[8..12], &[255, 0, 0, 255]);
        let t = c.texture(1).unwrap();
        assert_eq!(t.rgba, vec![0, 255, 0, 128]);
        // The same through the file entry point: page 2 is the second texture and the page count is set.
        let dir = std::env::temp_dir().join(format!("nv_unity_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("two.unity3d");
        std::fs::write(&f, &bytes).unwrap();
        let img = decode(&f, 2, true).unwrap();
        assert_eq!((img.width, img.page), (1, Some((2, 2))));
        let img = decode(&f, 1, true).unwrap();
        assert_eq!((img.width, img.page), (2, Some((1, 2))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Real bundles from a game's cache (skipped when the folder is not there): every one must unpack and give a picture.
    #[test]
    fn real_bundles_give_pictures() {
        let Some(home) = std::env::var_os("USERPROFILE") else { return };
        let dir = std::path::Path::new(&home).join("AppData/LocalLow/Watcher of Realms/Watcher of Realms/cacheimages");
        let Ok(rd) = std::fs::read_dir(&dir) else { return };
        let mut n = 0;
        for e in rd.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "unity3d")).take(40) {
            let bytes = std::fs::read(e.path()).unwrap();
            let c = open(&bytes).unwrap_or_else(|er| panic!("{}: {er}", e.path().display()));
            assert_eq!(c.textures.len(), 1, "{}", e.path().display());
            let t = c.texture(0).unwrap_or_else(|er| panic!("{}: {er}", e.path().display()));
            assert_eq!(t.rgba.len(), (t.width * t.height * 4) as usize);
            // A picture, not a flat fill or noise: some opaque and some varying pixels.
            let opaque = t.rgba.chunks_exact(4).filter(|p| p[3] > 0).count();
            assert!(opaque > 0, "{}: fully transparent", e.path().display());
            n += 1;
        }
        eprintln!("{n} real bundles decoded");
    }
}
