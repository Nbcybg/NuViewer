//! Direct3D 11 renderer: flip-model swapchain, sRGB-correct sampling, per-image mip chains,
//! Catmull-Rom resampling in the pixel shader, and a keyed GPU texture cache.

mod mesh;

pub use nv_codecs::model::{LightMode, ModelStyle, Orbit, Shading, UpAxis};
use nv_codecs::Image;
use std::collections::HashMap;
use std::ffi::c_void;
use windows::core::Interface;
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;

static VS_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/quad_vs.cso"));
static PS_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/quad_ps.cso"));

/// Largest texture dimension guaranteed by feature level 11_0 (shared with the decoder, which shrinks to fit).
const MAX_TEXTURE_DIM: u32 = nv_codecs::GPU_MAX_DIM;

struct Texture {
    /// One view per animation frame (a single entry for still images).
    frames: Vec<ID3D11ShaderResourceView>,
    /// Display time of each frame in ms (empty for still images).
    delays: Vec<u32>,
    /// Frame currently drawn.
    frame: usize,
    width: u32,
    height: u32,
    mips: u32,
    bytes: usize,
    /// EXIF orientation 1..=8 applied by the shader when drawing (1 = upright). Pixels stay as decoded.
    orientation: u8,
}

impl Texture {
    /// Size as displayed: width and height swap for the 90-degree orientations (5..=8).
    fn oriented_size(&self) -> (u32, u32) {
        if self.orientation >= 5 { (self.height, self.width) } else { (self.width, self.height) }
    }

    fn srv(&self) -> &ID3D11ShaderResourceView {
        &self.frames[self.frame.min(self.frames.len() - 1)]
    }
}

pub struct Renderer {
    device: ID3D11Device,
    ctx: ID3D11DeviceContext,
    swapchain: IDXGISwapChain1,
    rtv: Option<ID3D11RenderTargetView>,
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    cbuf: ID3D11Buffer,
    blend: ID3D11BlendState,
    /// 1x1 white texture used by `fill_rect`.
    white: ID3D11ShaderResourceView,
    /// Full-size viewer images (with mips), keyed by caller-chosen id.
    textures: HashMap<u64, Texture>,
    /// Gallery thumbnails (single mip), keyed by caller-chosen id.
    thumbs: HashMap<u64, Texture>,
    /// Text/HUD bitmaps (white + alpha), tinted when drawn. Separate from `thumbs` so gallery eviction can't touch them.
    overlays: HashMap<u64, Texture>,
    current: Option<u64>,
    size: (u32, u32),
    /// 3D model pipeline, created when the first model is uploaded.
    mesh: Option<mesh::MeshPipeline>,
    /// A big picture being copied to the GPU a slice at a time (see `begin_upload`).
    pending: Option<PendingUpload>,
    /// Dedicated video memory of the adapter, bytes (0 = unknown).
    vram: u64,
}

/// A picture on its way to the GPU in bands of rows, so a 100-megapixel upload never blocks the window for long.
struct PendingUpload {
    id: u64,
    img: Image,
    tag: f64,
    mips: u32,
    /// Frames finished so far.
    done: Vec<ID3D11ShaderResourceView>,
    /// The frame being filled: texture, view, next row to copy.
    cur: Option<(ID3D11Texture2D, ID3D11ShaderResourceView, u32)>,
    /// Total rows to copy over all frames, and rows copied so far (for the progress bar).
    rows_total: u64,
    rows_done: u64,
}

/// A picture that `step_upload` has finished: it is in the texture cache now.
pub struct UploadDone {
    pub id: u64,
    pub img: Image,
    pub tag: f64,
}

/// Pictures with fewer bytes than this upload in one go; bigger ones are copied in bands.
pub const STAGED_UPLOAD_BYTES: usize = 128 * 1024 * 1024;
/// Bytes copied per `UpdateSubresource` call when staging.
const BAND_BYTES: usize = 6 * 1024 * 1024;

fn err<T: std::fmt::Display>(what: &'static str) -> impl FnOnce(T) -> String {
    move |e| format!("{what}: {e}")
}

/// Immutable single-mip sRGB RGBA8 texture.
fn upload_immutable(device: &ID3D11Device, w: u32, h: u32, rgba: &[u8]) -> Result<(ID3D11Texture2D, ID3D11ShaderResourceView), String> {
    unsafe {
        let td = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_IMMUTABLE,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            ..Default::default()
        };
        let init = D3D11_SUBRESOURCE_DATA { pSysMem: rgba.as_ptr() as *const c_void, SysMemPitch: w * 4, SysMemSlicePitch: 0 };
        let mut tex = None;
        device.CreateTexture2D(&td, Some(&init), Some(&mut tex)).map_err(err("CreateTexture2D"))?;
        let tex: ID3D11Texture2D = tex.ok_or("texture")?;
        let mut srv = None;
        device.CreateShaderResourceView(&tex, None, Some(&mut srv)).map_err(err("CreateSRV"))?;
        Ok((tex, srv.ok_or("srv")?))
    }
}

/// A D3D11 device + immediate context, created without a window. Creating it is by far the slowest part of
/// startup (it loads the GPU driver, ~200+ ms), so the app does it on a background thread while it paints
/// the first image on the CPU, then hands it to `Renderer::from_device` on the UI thread.
pub struct GpuDevice {
    device: ID3D11Device,
    ctx: ID3D11DeviceContext,
}

// SAFETY: D3D11 devices are free-threaded; the immediate context is only *used* by whoever owns the
// resulting `Renderer` (the UI thread). It is merely moved between threads here.
unsafe impl Send for GpuDevice {}

pub fn create_device() -> Result<GpuDevice, String> {
    unsafe {
        let mut device = None;
        let mut ctx = None;
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut ctx),
        )
        .map_err(err("D3D11CreateDevice"))?;
        Ok(GpuDevice { device: device.ok_or("no device")?, ctx: ctx.ok_or("no context")? })
    }
}

impl Renderer {
    /// Convenience: create the device and renderer synchronously.
    pub fn new(hwnd: HWND, width: u32, height: u32) -> Result<Self, String> {
        Self::from_device(create_device()?, hwnd, width, height)
    }

    /// Finish setup (swapchain, shaders, state) on the calling thread; cheap compared to `create_device`.
    pub fn from_device(gpu: GpuDevice, hwnd: HWND, width: u32, height: u32) -> Result<Self, String> {
        let GpuDevice { device, ctx } = gpu;
        unsafe {
            let dxgi: IDXGIDevice = device.cast().map_err(err("IDXGIDevice"))?;
            let adapter = dxgi.GetAdapter().map_err(err("GetAdapter"))?;
            let vram = adapter.GetDesc().map(|d| d.DedicatedVideoMemory as u64).unwrap_or(0);
            let factory: IDXGIFactory2 = adapter.GetParent().map_err(err("GetParent"))?;
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: width.max(1),
                Height: height.max(1),
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                ..Default::default()
            };
            let swapchain = factory
                .CreateSwapChainForHwnd(&device, hwnd, &desc, None, None)
                .map_err(err("CreateSwapChainForHwnd"))?;

            let mut vs = None;
            device.CreateVertexShader(VS_BYTES, None, Some(&mut vs)).map_err(err("CreateVertexShader"))?;
            let mut ps = None;
            device.CreatePixelShader(PS_BYTES, None, Some(&mut ps)).map_err(err("CreatePixelShader"))?;

            let cb_desc = D3D11_BUFFER_DESC {
                ByteWidth: 96,
                Usage: D3D11_USAGE_DYNAMIC,
                BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                ..Default::default()
            };
            let mut cbuf = None;
            device.CreateBuffer(&cb_desc, None, Some(&mut cbuf)).map_err(err("CreateBuffer"))?;

            let mut bd = D3D11_BLEND_DESC::default();
            bd.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
                BlendEnable: true.into(),
                SrcBlend: D3D11_BLEND_SRC_ALPHA,
                DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
                BlendOp: D3D11_BLEND_OP_ADD,
                SrcBlendAlpha: D3D11_BLEND_ONE,
                DestBlendAlpha: D3D11_BLEND_ZERO,
                BlendOpAlpha: D3D11_BLEND_OP_ADD,
                RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
            };
            let mut blend = None;
            device.CreateBlendState(&bd, Some(&mut blend)).map_err(err("CreateBlendState"))?;

            let white_px = [255u8; 4];
            let white = upload_immutable(&device, 1, 1, &white_px)?.1;

            let mut r = Renderer {
                device,
                ctx,
                swapchain,
                rtv: None,
                vs: vs.ok_or("vs")?,
                ps: ps.ok_or("ps")?,
                cbuf: cbuf.ok_or("cbuf")?,
                blend: blend.ok_or("blend")?,
                white,
                textures: HashMap::new(),
                thumbs: HashMap::new(),
                overlays: HashMap::new(),
                current: None,
                size: (width.max(1), height.max(1)),
                mesh: None,
                pending: None,
                vram,
            };
            r.create_rtv()?;
            Ok(r)
        }
    }

    fn create_rtv(&mut self) -> Result<(), String> {
        unsafe {
            let back: ID3D11Texture2D = self.swapchain.GetBuffer(0).map_err(err("GetBuffer"))?;
            // sRGB view over a UNORM flip-model buffer => output is gamma-correct.
            let rd = D3D11_RENDER_TARGET_VIEW_DESC {
                Format: DXGI_FORMAT_B8G8R8A8_UNORM_SRGB,
                ViewDimension: D3D11_RTV_DIMENSION_TEXTURE2D,
                ..Default::default()
            };
            let mut rtv = None;
            self.device.CreateRenderTargetView(&back, Some(&rd), Some(&mut rtv)).map_err(err("CreateRenderTargetView"))?;
            self.rtv = rtv;
        }
        Ok(())
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        let (w, h) = (width.max(1), height.max(1));
        if (w, h) == self.size {
            return Ok(());
        }
        unsafe {
            self.ctx.OMSetRenderTargets(None, None);
            self.rtv = None;
            self.swapchain.ResizeBuffers(0, w, h, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0)).map_err(err("ResizeBuffers"))?;
        }
        self.size = (w, h);
        self.create_rtv()
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    // ---- texture cache -------------------------------------------------------------------------

    pub fn has(&self, id: u64) -> bool {
        self.textures.contains_key(&id)
    }

    /// Upload `img` (RGBA8) as a texture with a full mip chain under `id`.
    pub fn upload(&mut self, id: u64, img: &Image) -> Result<(), String> {
        if img.width > MAX_TEXTURE_DIM || img.height > MAX_TEXTURE_DIM {
            return Err(format!("image {}x{} exceeds GPU texture limit (tiling not implemented yet)", img.width, img.height));
        }
        let mips = 32 - img.width.max(img.height).leading_zeros();
        let mut frames = vec![self.upload_mipped(img.width, img.height, &img.rgba, mips)?];
        let mut delays = Vec::new();
        if !img.extra_frames.is_empty() {
            delays.push(img.delay_ms);
            for f in &img.extra_frames {
                frames.push(self.upload_mipped(img.width, img.height, &f.rgba, mips)?);
                delays.push(f.delay_ms);
            }
        }
        // Submit the copy + mip generation now. Otherwise the driver may hold the work back until the texture is
        // first drawn, so a prefetched image still costs tens of ms on the key press that shows it.
        unsafe { self.ctx.Flush() };
        let bytes = (img.width as usize * img.height as usize * 4) * 4 / 3 * frames.len();
        let orientation = if img.orientation >= 2 && img.orientation <= 8 && img.extra_frames.is_empty() { img.orientation } else { 1 };
        self.textures.insert(id, Texture { frames, delays, frame: 0, width: img.width, height: img.height, mips, bytes, orientation });
        Ok(())
    }

    /// True when `img` is big enough that it should be uploaded with `begin_upload` / `step_upload`.
    pub fn wants_staged_upload(img: &Image) -> bool {
        img.rgba.len().saturating_mul(img.frame_count()) >= STAGED_UPLOAD_BYTES
    }

    /// Start copying `img` to the GPU under `id`; `step_upload` does the work. Only one upload runs at a time:
    /// returns the picture back when another is still in progress. `tag` is handed back untouched with the result.
    pub fn begin_upload(&mut self, id: u64, img: Image, tag: f64) -> Result<(), (Image, String)> {
        if self.pending.is_some() {
            return Err((img, "another upload is in progress".into()));
        }
        if img.width > MAX_TEXTURE_DIM || img.height > MAX_TEXTURE_DIM {
            let e = format!("image {}x{} exceeds GPU texture limit (tiling not implemented yet)", img.width, img.height);
            return Err((img, e));
        }
        let mips = 32 - img.width.max(img.height).leading_zeros();
        let rows_total = img.height as u64 * img.frame_count() as u64;
        self.pending = Some(PendingUpload { id, img, tag, mips, done: Vec::new(), cur: None, rows_total, rows_done: 0 });
        Ok(())
    }

    /// Copy bands of the picture for up to `budget` (at least one band). Returns the picture once everything is on the
    /// GPU (it is cached by then), `None` while more remains.
    pub fn step_upload(&mut self, budget: std::time::Duration) -> Result<Option<UploadDone>, String> {
        let t0 = std::time::Instant::now();
        let Some(mut p) = self.pending.take() else { return Ok(None) };
        let w = p.img.width;
        let band_rows = (BAND_BYTES / (w as usize * 4).max(1)).clamp(1, p.img.height as usize) as u32;
        loop {
            if p.cur.is_none() {
                // On failure `pending` stays cleared; the caller reports the error.
                let t = self.create_mipped(w, p.img.height, p.mips)?;
                p.cur = Some((t.0, t.1, 0));
            }
            let frame_no = p.done.len();
            let (tex, srv, row) = p.cur.take().unwrap();
            let data: &[u8] = if frame_no == 0 { &p.img.rgba } else { &p.img.extra_frames[frame_no - 1].rgba };
            let rows = band_rows.min(p.img.height - row);
            let start = row as usize * w as usize * 4;
            let band = &data[start..start + rows as usize * w as usize * 4];
            let region = D3D11_BOX { left: 0, top: row, front: 0, right: w, bottom: row + rows, back: 1 };
            unsafe { self.ctx.UpdateSubresource(&tex, 0, Some(&region), band.as_ptr() as *const c_void, w * 4, 0) };
            p.rows_done += rows as u64;
            if row + rows >= p.img.height {
                unsafe { self.ctx.GenerateMips(&srv) };
                p.done.push(srv);
            } else {
                p.cur = Some((tex, srv, row + rows));
            }
            if p.done.len() == p.img.frame_count() {
                unsafe { self.ctx.Flush() };
                let PendingUpload { id, img, tag, mips, done, .. } = p;
                let delays = if img.extra_frames.is_empty() {
                    Vec::new()
                } else {
                    std::iter::once(img.delay_ms).chain(img.extra_frames.iter().map(|f| f.delay_ms)).collect()
                };
                let bytes = (img.width as usize * img.height as usize * 4) * 4 / 3 * done.len();
                let orientation = if (2..=8).contains(&img.orientation) && img.extra_frames.is_empty() { img.orientation } else { 1 };
                self.textures.insert(id, Texture { frames: done, delays, frame: 0, width: img.width, height: img.height, mips, bytes, orientation });
                return Ok(Some(UploadDone { id, img, tag }));
            }
            if t0.elapsed() >= budget {
                self.pending = Some(p);
                return Ok(None);
            }
        }
    }

    /// Id of the picture being uploaded in bands, with how far along it is (0..1).
    pub fn upload_progress(&self) -> Option<(u64, f32)> {
        self.pending.as_ref().map(|p| (p.id, (p.rows_done as f32 / p.rows_total.max(1) as f32).clamp(0.0, 1.0)))
    }

    /// Abandon the upload in progress and give the picture back (e.g. to make room for a more urgent one).
    pub fn cancel_upload(&mut self) -> Option<(u64, Image, f64)> {
        self.pending.take().map(|p| (p.id, p.img, p.tag))
    }

    /// Texture with a full mip chain, empty (to be filled by `UpdateSubresource`).
    fn create_mipped(&self, w: u32, h: u32, mips: u32) -> Result<(ID3D11Texture2D, ID3D11ShaderResourceView), String> {
        unsafe {
            let td = D3D11_TEXTURE2D_DESC {
                Width: w,
                Height: h,
                MipLevels: mips,
                ArraySize: 1,
                Format: DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32,
                MiscFlags: D3D11_RESOURCE_MISC_GENERATE_MIPS.0 as u32,
                ..Default::default()
            };
            let mut tex = None;
            self.device.CreateTexture2D(&td, None, Some(&mut tex)).map_err(err("CreateTexture2D"))?;
            let tex: ID3D11Texture2D = tex.ok_or("texture")?;
            let mut srv = None;
            self.device.CreateShaderResourceView(&tex, None, Some(&mut srv)).map_err(err("CreateSRV"))?;
            Ok((tex, srv.ok_or("srv")?))
        }
    }

    /// One mipmapped sRGB texture (GPU-generated mip chain).
    fn upload_mipped(&self, w: u32, h: u32, rgba: &[u8], mips: u32) -> Result<ID3D11ShaderResourceView, String> {
        unsafe {
            let td = D3D11_TEXTURE2D_DESC {
                Width: w,
                Height: h,
                MipLevels: mips,
                ArraySize: 1,
                Format: DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32,
                MiscFlags: D3D11_RESOURCE_MISC_GENERATE_MIPS.0 as u32,
                ..Default::default()
            };
            let mut tex = None;
            self.device.CreateTexture2D(&td, None, Some(&mut tex)).map_err(err("CreateTexture2D"))?;
            let tex: ID3D11Texture2D = tex.ok_or("texture")?;
            self.ctx.UpdateSubresource(&tex, 0, None, rgba.as_ptr() as *const c_void, w * 4, 0);
            let mut srv = None;
            self.device.CreateShaderResourceView(&tex, None, Some(&mut srv)).map_err(err("CreateSRV"))?;
            let srv: ID3D11ShaderResourceView = srv.ok_or("srv")?;
            self.ctx.GenerateMips(&srv);
            Ok(srv)
        }
    }

    /// Number of frames in texture `id` (1 for stills, 0 if not uploaded).
    pub fn frame_count(&self, id: u64) -> usize {
        self.textures.get(&id).map(|t| t.frames.len()).unwrap_or(0)
    }

    /// Index of the frame currently shown for `id`.
    pub fn frame_index(&self, id: u64) -> usize {
        self.textures.get(&id).map(|t| t.frame).unwrap_or(0)
    }

    /// Display time (ms) of the frame currently shown for `id`; None for stills.
    pub fn frame_delay(&self, id: u64) -> Option<u32> {
        self.textures.get(&id).and_then(|t| t.delays.get(t.frame).copied())
    }

    /// Show frame `frame` of `id` (clamped).
    pub fn set_frame(&mut self, id: u64, frame: usize) {
        if let Some(t) = self.textures.get_mut(&id) {
            t.frame = frame.min(t.frames.len().saturating_sub(1));
        }
    }

    /// Step `id` to its next frame (wrapping). Returns the new frame's delay in ms.
    pub fn advance_frame(&mut self, id: u64) -> Option<u32> {
        let t = self.textures.get_mut(&id)?;
        if t.frames.len() < 2 {
            return None;
        }
        t.frame = (t.frame + 1) % t.frames.len();
        t.delays.get(t.frame).copied()
    }

    /// Make `id` the displayed texture. Returns false if it isn't uploaded.
    pub fn show(&mut self, id: u64) -> bool {
        if self.textures.contains_key(&id) {
            self.current = Some(id);
            true
        } else {
            false
        }
    }

    /// Stop showing any full image (the textures stay cached): the viewer draws a placeholder until `show` is called.
    pub fn hide_current(&mut self) {
        self.current = None;
    }

    /// Drop every texture for which `keep` is false (the displayed one is always kept).
    pub fn retain(&mut self, keep: impl Fn(u64) -> bool) {
        let cur = self.current;
        self.textures.retain(|id, _| Some(*id) == cur || keep(*id));
        if let Some(m) = &mut self.mesh {
            m.meshes.retain(|id, _| Some(*id) == cur || keep(*id));
        }
    }

    /// Forget every cached image and thumbnail (a different folder was opened).
    pub fn clear_cache(&mut self) {
        self.textures.clear();
        self.thumbs.clear();
        if let Some(m) = &mut self.mesh {
            m.meshes.clear();
        }
        self.current = None;
    }

    /// Drop everything cached under `id` (full image and thumbnail), even if it is the displayed one.
    pub fn forget(&mut self, id: u64) {
        self.textures.remove(&id);
        self.thumbs.remove(&id);
        if let Some(m) = &mut self.mesh {
            m.meshes.remove(&id);
        }
        if self.current == Some(id) {
            self.current = None;
        }
    }

    /// Move whatever is cached under `old` to `new` (file renamed: same pixels, new identity).
    pub fn rekey(&mut self, old: u64, new: u64) {
        if let Some(t) = self.textures.remove(&old) {
            self.textures.insert(new, t);
        }
        if let Some(t) = self.thumbs.remove(&old) {
            self.thumbs.insert(new, t);
        }
        if self.current == Some(old) {
            self.current = Some(new);
        }
    }

    pub fn remove(&mut self, id: u64) {
        if self.current != Some(id) {
            self.textures.remove(&id);
            if let Some(m) = &mut self.mesh {
                m.meshes.remove(&id);
            }
        }
    }

    // ---- 3D models -----------------------------------------------------------------------------

    /// Upload a model's geometry under `id` (the pipeline is created on first use).
    pub fn upload_mesh(&mut self, id: u64, mesh: &std::sync::Arc<nv_codecs::model::Mesh>) -> Result<(), String> {
        if self.mesh.is_none() {
            self.mesh = Some(mesh::MeshPipeline::new(&self.device)?);
        }
        self.mesh.as_mut().ok_or("mesh pipeline")?.upload(&self.device, &self.ctx, id, mesh)
    }

    pub fn has_mesh(&self, id: u64) -> bool {
        self.mesh.as_ref().map(|m| m.meshes.contains_key(&id)).unwrap_or(false)
    }

    /// Draw model `id` filling the frame (call between `begin` and the 2D overlays). Returns false if it isn't uploaded.
    pub fn draw_mesh(&mut self, id: u64, orbit: &Orbit, style: &ModelStyle) -> Result<bool, String> {
        let Some(rtv) = self.rtv.clone() else { return Ok(false) };
        let Some(pipe) = self.mesh.as_mut() else { return Ok(false) };
        let drawn = pipe.draw(&self.device, &self.ctx, &rtv, self.size, id, orbit, style)?;
        if drawn {
            // Back to the 2D state `begin` set up.
            unsafe {
                self.ctx.OMSetRenderTargets(Some(&[Some(rtv)]), None);
                self.ctx.OMSetDepthStencilState(None, 0);
                self.ctx.RSSetState(None);
                self.ctx.IASetInputLayout(None);
                self.ctx.OMSetBlendState(&self.blend, None, 0xFFFF_FFFF);
                self.ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
                self.ctx.VSSetShader(&self.vs, None);
                self.ctx.VSSetConstantBuffers(0, Some(&[Some(self.cbuf.clone())]));
                self.ctx.PSSetShader(&self.ps, None);
                self.ctx.PSSetConstantBuffers(0, Some(&[Some(self.cbuf.clone())]));
            }
        }
        Ok(drawn)
    }

    // ---- thumbnail cache -----------------------------------------------------------------------

    pub fn has_thumb(&self, id: u64) -> bool {
        self.thumbs.contains_key(&id)
    }

    pub fn upload_thumb(&mut self, id: u64, img: &Image) -> Result<(), String> {
        let (_tex, srv) = upload_immutable(&self.device, img.width, img.height, &img.rgba)?;
        let bytes = img.width as usize * img.height as usize * 4;
        self.thumbs.insert(id, Texture { frames: vec![srv], delays: Vec::new(), frame: 0, width: img.width, height: img.height, mips: 1, bytes, orientation: 1 });
        Ok(())
    }

    pub fn retain_thumbs(&mut self, keep: impl Fn(u64) -> bool) {
        self.thumbs.retain(|id, _| keep(*id));
    }

    pub fn thumb_size(&self, id: u64) -> Option<(u32, u32)> {
        self.thumbs.get(&id).map(|t| (t.width, t.height))
    }

    // ---- overlays (text) ---------------------------------------------------------------------

    pub fn has_overlay(&self, id: u64) -> bool {
        self.overlays.contains_key(&id)
    }

    pub fn upload_overlay(&mut self, id: u64, img: &Image) -> Result<(), String> {
        let (_tex, srv) = upload_immutable(&self.device, img.width, img.height, &img.rgba)?;
        let bytes = img.width as usize * img.height as usize * 4;
        self.overlays.insert(id, Texture { frames: vec![srv], delays: Vec::new(), frame: 0, width: img.width, height: img.height, mips: 1, bytes, orientation: 1 });
        Ok(())
    }

    pub fn overlay_size(&self, id: u64) -> Option<(u32, u32)> {
        self.overlays.get(&id).map(|t| (t.width, t.height))
    }

    /// Drop overlays for which `keep` is false.
    pub fn retain_overlays(&mut self, keep: impl Fn(u64) -> bool) {
        self.overlays.retain(|id, _| keep(*id));
    }

    /// Draw an overlay at `x`,`y` (client pixels, 1:1) multiplied by `tint` (linear RGBA).
    pub fn draw_overlay(&mut self, id: u64, x: f32, y: f32, tint: [f32; 4]) -> Result<(), String> {
        let Some((srv, tw, th)) = self.overlays.get(&id).map(|t| (t.srv().clone(), t.width, t.height)) else { return Ok(()) };
        let (x, y) = (x.round(), y.round());
        self.draw_quad(&srv, [x, y, x + tw as f32, y + th as f32], [tw as f32, th as f32, 0.0, 1.0], tint, [0.0; 4])
    }

    pub fn thumb_count(&self) -> usize {
        self.thumbs.len()
    }

    pub fn current(&self) -> Option<u64> {
        self.current
    }

    /// Dedicated video memory of the graphics card in bytes (0 if unknown).
    pub fn video_memory(&self) -> u64 {
        self.vram
    }

    /// GPU bytes held by the cached picture `id` (None if it isn't uploaded).
    pub fn texture_size(&self, id: u64) -> Option<usize> {
        self.textures.get(&id).map(|t| t.bytes)
    }

    pub fn texture_bytes(&self) -> usize {
        self.textures.values().map(|t| t.bytes).sum()
    }

    /// Size of the displayed texture.
    pub fn image_size(&self) -> Option<(u32, u32)> {
        self.current.and_then(|id| self.textures.get(&id)).map(|t| t.oriented_size())
    }

    // ---- drawing -------------------------------------------------------------------------------
    //
    // A frame is: begin() -> any number of draw_*/fill_rect() -> end() (presents).

    /// Start a frame: bind the render target and pipeline state, and clear.
    pub fn begin(&mut self, clear: [f32; 4]) -> bool {
        unsafe {
            let Some(rtv) = self.rtv.clone() else { return false };
            let (w, h) = (self.size.0 as f32, self.size.1 as f32);
            self.ctx.OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
            let vp = D3D11_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: w, Height: h, MinDepth: 0.0, MaxDepth: 1.0 };
            self.ctx.RSSetViewports(Some(&[vp]));
            self.ctx.ClearRenderTargetView(&rtv, &clear);
            self.ctx.OMSetBlendState(&self.blend, None, 0xFFFF_FFFF);
            self.ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
            self.ctx.VSSetShader(&self.vs, None);
            self.ctx.VSSetConstantBuffers(0, Some(&[Some(self.cbuf.clone())]));
            self.ctx.PSSetShader(&self.ps, None);
            self.ctx.PSSetConstantBuffers(0, Some(&[Some(self.cbuf.clone())]));
        }
        true
    }

    /// Present the frame (vsync). DXGI_STATUS_OCCLUDED is a success code, so `is_err()` ignores it.
    pub fn end(&mut self) -> Result<(), String> {
        let hr = unsafe { self.swapchain.Present(1, DXGI_PRESENT(0)) };
        if hr.is_err() {
            return Err(format!("Present: {hr:?}"));
        }
        Ok(())
    }

    fn draw_quad(&mut self, srv: &ID3D11ShaderResourceView, dst: [f32; 4], params: [f32; 4], tint: [f32; 4], extra: [f32; 4]) -> Result<(), String> {
        self.draw_quad_turned(srv, dst, params, tint, extra, 0.0)
    }

    /// `draw_quad` with the rectangle turned clockwise by `degrees` about its centre.
    fn draw_quad_turned(&mut self, srv: &ID3D11ShaderResourceView, dst: [f32; 4], params: [f32; 4], tint: [f32; 4], extra: [f32; 4], degrees: f32) -> Result<(), String> {
        unsafe {
            let (w, h) = (self.size.0 as f32, self.size.1 as f32);
            // Clockwise on screen is counter-clockwise with y up; right angles are made exact so a quarter turn stays pixel-sharp.
            let a = (-degrees).to_radians();
            let snap = |v: f32| if v.abs() < 1e-6 { 0.0 } else if (v.abs() - 1.0).abs() < 1e-6 { v.signum() } else { v };
            let rot = [snap(a.cos()), snap(a.sin()), w, h];
            let ndc = [dst[0] / w * 2.0 - 1.0, 1.0 - dst[1] / h * 2.0, dst[2] / w * 2.0 - 1.0, 1.0 - dst[3] / h * 2.0];
            let mut m = D3D11_MAPPED_SUBRESOURCE::default();
            self.ctx.Map(&self.cbuf, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut m)).map_err(err("Map"))?;
            let p = m.pData as *mut f32;
            std::ptr::copy_nonoverlapping(ndc.as_ptr(), p, 4);
            std::ptr::copy_nonoverlapping(params.as_ptr(), p.add(4), 4);
            std::ptr::copy_nonoverlapping(tint.as_ptr(), p.add(8), 4);
            std::ptr::copy_nonoverlapping(extra.as_ptr(), p.add(12), 4);
            std::ptr::copy_nonoverlapping(rot.as_ptr(), p.add(16), 4);
            self.ctx.Unmap(&self.cbuf, 0);
            self.ctx.PSSetShaderResources(0, Some(&[Some(srv.clone())]));
            self.ctx.Draw(4, 0);
        }
        Ok(())
    }

    /// Draw the displayed viewer image into `dst` = [left, top, right, bottom] in client pixels.
    /// `nearest` selects point sampling (for inspecting pixels when zoomed in far).
    pub fn draw_current(&mut self, dst: [f32; 4], nearest: bool) -> Result<(), String> {
        self.draw_current_turned(dst, nearest, 0.0)
    }

    /// `draw_current`, with the picture turned clockwise by `degrees` about the centre of `dst`. `dst` is the rectangle
    /// of the *unturned* picture (so for a quarter turn it is the picture's own shape, not the turned one).
    pub fn draw_current_turned(&mut self, dst: [f32; 4], nearest: bool, degrees: f32) -> Result<(), String> {
        match self.current {
            Some(id) => self.draw_image_turned(id, dst, nearest, degrees),
            None => Ok(()),
        }
    }

    /// Draw the cached picture `id` (not necessarily the displayed one) into `dst`; nothing happens if it isn't uploaded.
    /// Read mode uses this to put two pages side by side.
    pub fn draw_image(&mut self, id: u64, dst: [f32; 4], nearest: bool) -> Result<(), String> {
        self.draw_image_turned(id, dst, nearest, 0.0)
    }

    /// Displayed size of the cached picture `id` (its EXIF orientation applied).
    pub fn image_size_of(&self, id: u64) -> Option<(u32, u32)> {
        self.textures.get(&id).map(|t| t.oriented_size())
    }

    fn draw_image_turned(&mut self, id: u64, dst: [f32; 4], nearest: bool, degrees: f32) -> Result<(), String> {
        let Some((srv, tw, th, mips, orientation, ow)) = self.textures.get(&id).map(|t| (t.srv().clone(), t.width, t.height, t.mips, t.orientation, t.oriented_size().0)) else {
            return Ok(());
        };
        // Pick the mip so that the remaining downscale factor is in [0.5, 1).
        let scale = (dst[2] - dst[0]) / ow as f32; // scale of the *displayed* (oriented) image
        let lod = if scale < 1.0 { ((-scale.log2()).floor().max(0.0) as u32).min(mips - 1) } else { 0 };
        self.draw_quad_turned(&srv, dst, [tw as f32, th as f32, lod as f32, if nearest { 1.0 } else { 0.0 }], [1.0; 4], [orientation as f32, 0.0, 0.0, 0.0], degrees)
    }

    /// Draw a gallery thumbnail. Returns Ok(false) if it isn't uploaded.
    pub fn draw_thumb(&mut self, id: u64, dst: [f32; 4]) -> Result<bool, String> {
        let Some((srv, tw, th)) = self.thumbs.get(&id).map(|t| (t.srv().clone(), t.width, t.height)) else { return Ok(false) };
        // Thumbnails are drawn at <= 1:1, so the base mip with bicubic is enough; snap-to-pixel is done by the caller.
        self.draw_quad(&srv, dst, [tw as f32, th as f32, 0.0, 0.0], [1.0; 4], [0.0; 4])?;
        Ok(true)
    }

    /// Solid rectangle; `color` is linear RGBA (alpha blended).
    pub fn fill_rect(&mut self, dst: [f32; 4], color: [f32; 4]) -> Result<(), String> {
        let white = self.white.clone();
        self.draw_quad(&white, dst, [1.0, 1.0, 0.0, 1.0], color, [0.0; 4])
    }

    /// Convenience for the viewer: one frame showing the current image (or just the background).
    pub fn draw(&mut self, dst: Option<[f32; 4]>, clear: [f32; 4], nearest: bool) -> Result<(), String> {
        if !self.begin(clear) {
            return Ok(());
        }
        if let Some(d) = dst {
            self.draw_current(d, nearest)?;
        }
        self.end()
    }
}
