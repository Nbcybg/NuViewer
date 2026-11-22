# Unity asset bundles (`.unity3d .unityfs .bundle .ab .assetbundle`)

Code: `crates/nv-codecs/src/unityfs.rs`; tool: `cargo run --release -p nv-codecs --example unityinfo -- <bundle|folder> [--png out.png] [--tex N]`; tests: unit tests in `unityfs.rs` (a bundle built in the test + real files), `tools/test/e2e-unity.ps1`.

## What it does

- The first **Texture2D** of the bundle is the picture (viewer, gallery thumbnail, NuEditor as a flattened picture). More textures are **pages** (`file.unity3d?page=2`, next / previous like PDF pages). A bundle without a texture is a card listing what it holds (classes and counts).
- *Extract frames* (`Ctrl+Shift+F`) writes every texture of a bundle as a numbered picture (transparency forces PNG, as for animations).
- Gallery group "Unity bundles"; file associations list "Unity asset bundles".

## Format facts (checked on 122 real files: Unity 2019.4.40f1, StandaloneWindows64)

- `UnityFS` header (big endian): version 6/7/8, player and engine strings, size, compressed / unpacked size of the block table, flags. Flags: `& 0x3F` = compression of the **table** (0 none, 1 LZMA, 2 / 3 LZ4), `0x40` table and directory combined, `0x80` table at the **end of the file**. (`0x40` is *not* "table at the end": that mix-up cost an hour.) The table follows the header after a 16-byte alignment (version >= 7).
- Table: 16 byte hash, blocks (unpacked u32, packed u32, flags u16; the block's own compression is `flags & 0x3F`: the real files use **LZMA blocks inside an LZ4HC table**), then the files (offset i64, size i64, flags u32, name). Flag 4 = a serialized file; the others are `.resS` streams.
- LZMA in Unity = 5 property bytes (props, dictionary) then the raw stream, no length: `lzma_rs` with `UnpackedSize::UseProvided(Some(n))`. LZ4 is an own 40-line block decoder.
- The game's cache files have **24 extra bytes** after the end the header names (a trailer of the game's cache); the reader ignores them.
- Serialized file 14 - 22 (Unity 2017 - 2022; version 22 is read from the spec and **not tried on a real file**). Types carry a **type tree** (blob form, strings from the file or from Unity's built-in table, `COMMON_STRINGS`, offsets checked in a test). Objects are read through the type tree into `Val` values, so no per-Unity-version layouts are needed. A file without a type tree (stripped) is refused with a plain message.
- Texture2D: `m_Width`, `m_Height`, `m_TextureFormat`, `image data` or `m_StreamData {offset, size, path}` (the `.resS` named by the path's last part). Rows are bottom-up in Unity: flipped on reading.

## Texture formats

| Format | Reader | Checked |
|---|---|---|
| DXT1 (10), BC7 (25), RGBA32 (4) | `image_dds` / copy | real files (60 / 59 / 3 of 122) |
| DXT5 (12), BC4 / BC5 / BC6H (26 / 27 / 24) | `image_dds` | not on real files |
| Alpha8, RGB24, ARGB32, BGRA32, RGB565, RGBA4444, ARGB4444, R8, R16, RG16 | own, a few lines each | unit tests (565, ARGB32, BGRA32) |
| ETC / ETC2 / EAC (34, 41 - 47), ASTC 4x4 - 12x12 (48 - 59), PVRTC (30 - 33), Crunch (28, 29, 64, 65) | `texture2ddecoder` crate | ASTC void-extent block and an ETC2 block only; **no real file** |
| Half / float formats, ASTC HDR, cube maps / volumes | not read | error names the format |

## Not done

- `UnityWeb` / `UnityRaw` bundles (Unity 3 - 4): refused with a message. Encrypted or scrambled bundles (some games): the block table fails to unpack and the error says so; no attempt is made to get round it.
- Sprites (class 213) are not cut out of their texture; the whole texture is shown. SpriteAtlas, meshes, audio, TextAssets, animations: counted on the card only.
- Dependencies between bundles (a texture in another bundle) are not followed: `m_StreamData` must point into the same bundle.
- Mipmaps: only the top level is used.
