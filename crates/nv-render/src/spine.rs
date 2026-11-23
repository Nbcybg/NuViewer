//! Spine skeletons on the GPU. The caller poses the skeleton on the CPU (cheap: a millisecond or two) and hands over screen-space triangles;
//! this draws them into a texture of the window's size with 4x multisampling, atlas pages with mip maps, and the four Spine blend modes,
//! then the ordinary picture path shows that texture 1:1. The numbers match the CPU rasteriser (`nv-anim`): colours are blended as they
//! are (gamma space), straight pages are premultiplied on upload, a two-colour tint is done in the pixel shader.

use super::{err, Renderer, Texture};
use std::ffi::c_void;
use windows::core::PCSTR;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;

static SPINE_VS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/spine_vs.cso"));
static SPINE_PS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/spine_ps.cso"));

/// One vertex: position in target pixels (y down), texture coordinates, the light colour (straight RGBA as Spine has it), and the dark
/// colour with the mode in `w` (0 one colour, 1 two colours on a premultiplied page, 2 two colours on a straight page).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SpineVertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub light: [f32; 4],
    pub dark: [f32; 4],
}

/// A run of indices drawn with one page and one blend mode (0 normal, 1 additive, 2 multiply, 3 screen).
#[derive(Clone, Copy, Debug)]
pub struct SpineBatch {
    pub page: u32,
    pub blend: u8,
    pub first: u32,
    pub count: u32,
}

/// What `spine_set_atlas` takes per page: size, straight RGBA8 bytes, and whether they are already multiplied by alpha.
pub struct SpinePage<'a> {
    pub width: u32,
    pub height: u32,
    pub rgba: &'a [u8],
    pub premultiplied: bool,
}

struct Target {
    msaa: Option<(ID3D11Texture2D, ID3D11RenderTargetView)>,
    /// What is drawn into when there is no multisampling, and what the multisampled one is resolved to.
    resolved: ID3D11Texture2D,
    rtv: ID3D11RenderTargetView,
    srv: ID3D11ShaderResourceView,
    size: (u32, u32),
}

pub(crate) struct SpineGpu {
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    layout: ID3D11InputLayout,
    cbuf: ID3D11Buffer,
    sampler: ID3D11SamplerState,
    raster: ID3D11RasterizerState,
    blends: [ID3D11BlendState; 4],
    samples: u32,
    vb: Option<(ID3D11Buffer, usize)>,
    ib: Option<(ID3D11Buffer, usize)>,
    /// The atlas pages of the skeleton that was set last, with the key it was set under.
    atlas: Option<(u64, Vec<Option<ID3D11ShaderResourceView>>)>,
    targets: std::collections::HashMap<u64, Target>,
}

const STRIDE: u32 = std::mem::size_of::<SpineVertex>() as u32;

impl SpineGpu {
    fn new(device: &ID3D11Device) -> Result<SpineGpu, String> {
        unsafe {
            let mut vs = None;
            device.CreateVertexShader(SPINE_VS, None, Some(&mut vs)).map_err(err("spine vertex shader"))?;
            let mut ps = None;
            device.CreatePixelShader(SPINE_PS, None, Some(&mut ps)).map_err(err("spine pixel shader"))?;
            let el = |name: &'static [u8], index: u32, format: DXGI_FORMAT, offset: u32| D3D11_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR(name.as_ptr()),
                SemanticIndex: index,
                Format: format,
                InputSlot: 0,
                AlignedByteOffset: offset,
                InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            };
            let elements = [
                el(b"POSITION\0", 0, DXGI_FORMAT_R32G32_FLOAT, 0),
                el(b"TEXCOORD\0", 0, DXGI_FORMAT_R32G32_FLOAT, 8),
                el(b"COLOR\0", 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 16),
                el(b"COLOR\0", 1, DXGI_FORMAT_R32G32B32A32_FLOAT, 32),
            ];
            let mut layout = None;
            device.CreateInputLayout(&elements, SPINE_VS, Some(&mut layout)).map_err(err("spine input layout"))?;
            let cb = D3D11_BUFFER_DESC { ByteWidth: 16, Usage: D3D11_USAGE_DYNAMIC, BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32, CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32, ..Default::default() };
            let mut cbuf = None;
            device.CreateBuffer(&cb, None, Some(&mut cbuf)).map_err(err("spine constants"))?;
            let sd = D3D11_SAMPLER_DESC {
                Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                ComparisonFunc: D3D11_COMPARISON_NEVER,
                MinLOD: 0.0,
                // Only the first mip level down: it smooths a skeleton shown smaller than its pages, while the coarser ones would mix in
                // neighbours of other regions of the atlas (a big plate cut from a tiny region would show their colours).
                MaxLOD: 1.0,
                ..Default::default()
            };
            let mut sampler = None;
            device.CreateSamplerState(&sd, Some(&mut sampler)).map_err(err("spine sampler"))?;
            let rd = D3D11_RASTERIZER_DESC { FillMode: D3D11_FILL_SOLID, CullMode: D3D11_CULL_NONE, DepthClipEnable: true.into(), MultisampleEnable: true.into(), ..Default::default() };
            let mut raster = None;
            device.CreateRasterizerState(&rd, Some(&mut raster)).map_err(err("spine rasterizer"))?;
            // The shader gives premultiplied colour: normal = one / 1-src.a, additive = one / one, multiply = dst.rgb / 1-src.a, screen = one / 1-src.rgb.
            let make = |src: D3D11_BLEND, dst: D3D11_BLEND, src_a: D3D11_BLEND, dst_a: D3D11_BLEND| -> Result<ID3D11BlendState, String> {
                let mut bd = D3D11_BLEND_DESC::default();
                bd.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
                    BlendEnable: true.into(),
                    SrcBlend: src,
                    DestBlend: dst,
                    BlendOp: D3D11_BLEND_OP_ADD,
                    SrcBlendAlpha: src_a,
                    DestBlendAlpha: dst_a,
                    BlendOpAlpha: D3D11_BLEND_OP_ADD,
                    RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
                };
                let mut s = None;
                device.CreateBlendState(&bd, Some(&mut s)).map_err(err("spine blend"))?;
                s.ok_or_else(|| "spine blend".to_string())
            };
            let blends = [
                make(D3D11_BLEND_ONE, D3D11_BLEND_INV_SRC_ALPHA, D3D11_BLEND_ONE, D3D11_BLEND_INV_SRC_ALPHA)?,
                make(D3D11_BLEND_ONE, D3D11_BLEND_ONE, D3D11_BLEND_ONE, D3D11_BLEND_INV_SRC_ALPHA)?,
                make(D3D11_BLEND_DEST_COLOR, D3D11_BLEND_INV_SRC_ALPHA, D3D11_BLEND_ZERO, D3D11_BLEND_ONE)?,
                make(D3D11_BLEND_ONE, D3D11_BLEND_INV_SRC_COLOR, D3D11_BLEND_ONE, D3D11_BLEND_INV_SRC_ALPHA)?,
            ];
            // 4x multisampling when the card does it for this format (all feature-level 11 cards do).
            let q = device.CheckMultisampleQualityLevels(DXGI_FORMAT_R8G8B8A8_UNORM, 4).unwrap_or(0);
            Ok(SpineGpu {
                vs: vs.ok_or("spine vs")?,
                ps: ps.ok_or("spine ps")?,
                layout: layout.ok_or("spine layout")?,
                cbuf: cbuf.ok_or("spine cbuf")?,
                sampler: sampler.ok_or("spine sampler")?,
                raster: raster.ok_or("spine raster")?,
                blends,
                samples: if q > 0 { 4 } else { 1 },
                vb: None,
                ib: None,
                atlas: None,
                targets: std::collections::HashMap::new(),
            })
        }
    }
}

/// Grow (or create) a dynamic buffer to hold `need` bytes.
fn ensure_buffer(device: &ID3D11Device, slot: &mut Option<(ID3D11Buffer, usize)>, need: usize, bind: D3D11_BIND_FLAG, what: &'static str) -> Result<ID3D11Buffer, String> {
    if slot.as_ref().map(|(_, cap)| *cap < need).unwrap_or(true) {
        let cap = need.next_power_of_two().max(256 * 1024);
        let desc = D3D11_BUFFER_DESC { ByteWidth: cap as u32, Usage: D3D11_USAGE_DYNAMIC, BindFlags: bind.0 as u32, CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32, ..Default::default() };
        let mut b = None;
        unsafe { device.CreateBuffer(&desc, None, Some(&mut b)).map_err(err(what))? };
        *slot = Some((b.ok_or(what)?, cap));
    }
    Ok(slot.as_ref().unwrap().0.clone())
}

impl Renderer {
    fn spine_gpu(&mut self) -> Result<&mut SpineGpu, String> {
        if self.spine.is_none() {
            self.spine = Some(SpineGpu::new(&self.device)?);
        }
        Ok(self.spine.as_mut().unwrap())
    }

    /// True when the atlas pages of the skeleton `key` are on the GPU already.
    pub fn spine_has_atlas(&self, key: u64) -> bool {
        self.spine.as_ref().and_then(|s| s.atlas.as_ref()).map(|(k, _)| *k == key).unwrap_or(false)
    }

    /// Put the atlas pages of a skeleton on the GPU (premultiplied, with a full mip chain). Replaces the pages of the one before.
    pub fn spine_set_atlas(&mut self, key: u64, pages: &[Option<SpinePage>]) -> Result<(), String> {
        self.spine_gpu()?;
        let mut views: Vec<Option<ID3D11ShaderResourceView>> = Vec::with_capacity(pages.len());
        for page in pages {
            let Some(p) = page else {
                views.push(None);
                continue;
            };
            if p.width == 0 || p.height == 0 || p.width > super::MAX_TEXTURE_DIM || p.height > super::MAX_TEXTURE_DIM || p.rgba.len() < p.width as usize * p.height as usize * 4 {
                views.push(None);
                continue;
            }
            let mips = 32 - p.width.max(p.height).leading_zeros();
            unsafe {
                let td = D3D11_TEXTURE2D_DESC {
                    Width: p.width,
                    Height: p.height,
                    MipLevels: mips,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32,
                    MiscFlags: D3D11_RESOURCE_MISC_GENERATE_MIPS.0 as u32,
                    ..Default::default()
                };
                let mut tex = None;
                self.device.CreateTexture2D(&td, None, Some(&mut tex)).map_err(err("spine page"))?;
                let tex: ID3D11Texture2D = tex.ok_or("spine page")?;
                let n = p.width as usize * p.height as usize * 4;
                if p.premultiplied {
                    self.ctx.UpdateSubresource(&tex, 0, None, p.rgba.as_ptr() as *const c_void, p.width * 4, 0);
                } else {
                    let mut pm = vec![0u8; n];
                    for (o, i) in pm.chunks_exact_mut(4).zip(p.rgba[..n].chunks_exact(4)) {
                        let a = i[3] as u32;
                        o[0] = ((i[0] as u32 * a + 127) / 255) as u8;
                        o[1] = ((i[1] as u32 * a + 127) / 255) as u8;
                        o[2] = ((i[2] as u32 * a + 127) / 255) as u8;
                        o[3] = i[3];
                    }
                    self.ctx.UpdateSubresource(&tex, 0, None, pm.as_ptr() as *const c_void, p.width * 4, 0);
                }
                let mut srv = None;
                self.device.CreateShaderResourceView(&tex, None, Some(&mut srv)).map_err(err("spine page view"))?;
                let srv: ID3D11ShaderResourceView = srv.ok_or("spine page view")?;
                self.ctx.GenerateMips(&srv);
                views.push(Some(srv));
            }
        }
        self.spine_gpu()?.atlas = Some((key, views));
        Ok(())
    }

    /// Draw the triangles into the picture `id` (a texture of `size`, made when it is not there or has another size) over the opaque colour
    /// `bg` (straight RGB, what the picture view shows behind a picture). Afterwards `show(id)` displays it.
    pub fn spine_draw(&mut self, key: u64, id: u64, size: (u32, u32), bg: [f32; 3], verts: &[SpineVertex], indices: &[u32], batches: &[SpineBatch]) -> Result<(), String> {
        if size.0 == 0 || size.1 == 0 || size.0 > super::MAX_TEXTURE_DIM || size.1 > super::MAX_TEXTURE_DIM {
            return Err(format!("spine target {}x{} can't be made", size.0, size.1));
        }
        let (device, ctx) = (self.device.clone(), self.ctx.clone());
        let gpu = self.spine_gpu()?;
        if !matches!(&gpu.atlas, Some((k, _)) if *k == key) {
            return Err("the atlas of this skeleton is not on the GPU".into());
        }
        // Target (re)made when the size changed.
        if gpu.targets.get(&id).map(|t| t.size != size).unwrap_or(true) {
            unsafe {
                let base = D3D11_TEXTURE2D_DESC {
                    Width: size.0,
                    Height: size.1,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_R8G8B8A8_TYPELESS,
                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32,
                    ..Default::default()
                };
                let mut resolved = None;
                device.CreateTexture2D(&base, None, Some(&mut resolved)).map_err(err("spine target"))?;
                let resolved: ID3D11Texture2D = resolved.ok_or("spine target")?;
                let rtv_desc = D3D11_RENDER_TARGET_VIEW_DESC { Format: DXGI_FORMAT_R8G8B8A8_UNORM, ViewDimension: D3D11_RTV_DIMENSION_TEXTURE2D, ..Default::default() };
                let mut rtv = None;
                device.CreateRenderTargetView(&resolved, Some(&rtv_desc), Some(&mut rtv)).map_err(err("spine target view"))?;
                // Displayed as sRGB like every picture: the picture path decodes and the swap chain encodes again, so the bytes come out unchanged.
                let srv_desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
                    Format: DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
                    ViewDimension: D3D_SRV_DIMENSION_TEXTURE2D,
                    Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_SRV { MostDetailedMip: 0, MipLevels: 1 } },
                };
                let mut srv = None;
                device.CreateShaderResourceView(&resolved, Some(&srv_desc), Some(&mut srv)).map_err(err("spine target srv"))?;
                let msaa = if gpu.samples > 1 {
                    let md = D3D11_TEXTURE2D_DESC { SampleDesc: DXGI_SAMPLE_DESC { Count: gpu.samples, Quality: 0 }, BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32, ..base };
                    let mut mt = None;
                    device.CreateTexture2D(&md, None, Some(&mut mt)).map_err(err("spine msaa target"))?;
                    let mt: ID3D11Texture2D = mt.ok_or("spine msaa target")?;
                    let md_view = D3D11_RENDER_TARGET_VIEW_DESC { Format: DXGI_FORMAT_R8G8B8A8_UNORM, ViewDimension: D3D11_RTV_DIMENSION_TEXTURE2DMS, ..Default::default() };
                    let mut mrtv = None;
                    device.CreateRenderTargetView(&mt, Some(&md_view), Some(&mut mrtv)).map_err(err("spine msaa view"))?;
                    Some((mt, mrtv.ok_or("spine msaa view")?))
                } else {
                    None
                };
                gpu.targets.insert(id, Target { msaa, resolved, rtv: rtv.ok_or("spine target view")?, srv: srv.ok_or("spine target srv")?, size });
            }
        }
        // Geometry.
        let vbytes = std::mem::size_of_val(verts);
        let ibytes = std::mem::size_of_val(indices);
        let vb = ensure_buffer(&device, &mut gpu.vb, vbytes.max(16), D3D11_BIND_VERTEX_BUFFER, "spine vertices")?;
        let ib = ensure_buffer(&device, &mut gpu.ib, ibytes.max(16), D3D11_BIND_INDEX_BUFFER, "spine indices")?;
        let target = gpu.targets.get(&id).unwrap();
        let (draw_rtv, resolve) = match &target.msaa {
            Some((_, rtv)) => (rtv.clone(), true),
            None => (target.rtv.clone(), false),
        };
        unsafe {
            let mut m = D3D11_MAPPED_SUBRESOURCE::default();
            ctx.Map(&vb, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut m)).map_err(err("Map"))?;
            std::ptr::copy_nonoverlapping(verts.as_ptr() as *const u8, m.pData as *mut u8, vbytes);
            ctx.Unmap(&vb, 0);
            ctx.Map(&ib, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut m)).map_err(err("Map"))?;
            std::ptr::copy_nonoverlapping(indices.as_ptr() as *const u8, m.pData as *mut u8, ibytes);
            ctx.Unmap(&ib, 0);
            let consts = [size.0 as f32, size.1 as f32, 0.0, 0.0];
            ctx.Map(&gpu.cbuf, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut m)).map_err(err("Map"))?;
            std::ptr::copy_nonoverlapping(consts.as_ptr(), m.pData as *mut f32, 4);
            ctx.Unmap(&gpu.cbuf, 0);

            ctx.OMSetRenderTargets(Some(&[Some(draw_rtv.clone())]), None);
            let vp = D3D11_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: size.0 as f32, Height: size.1 as f32, MinDepth: 0.0, MaxDepth: 1.0 };
            ctx.RSSetViewports(Some(&[vp]));
            ctx.RSSetState(&gpu.raster);
            ctx.ClearRenderTargetView(&draw_rtv, &[bg[0], bg[1], bg[2], 1.0]);
            ctx.IASetInputLayout(&gpu.layout);
            ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            ctx.IASetVertexBuffers(0, 1, Some(&Some(vb.clone())), Some(&STRIDE), Some(&0u32));
            ctx.IASetIndexBuffer(&ib, DXGI_FORMAT_R32_UINT, 0);
            ctx.VSSetShader(&gpu.vs, None);
            ctx.VSSetConstantBuffers(0, Some(&[Some(gpu.cbuf.clone())]));
            ctx.PSSetShader(&gpu.ps, None);
            ctx.PSSetSamplers(0, Some(&[Some(gpu.sampler.clone())]));
            let pages = &gpu.atlas.as_ref().unwrap().1;
            let mut last: (i64, i64) = (-1, -1);
            for b in batches {
                if b.count == 0 {
                    continue;
                }
                let Some(Some(page)) = pages.get(b.page as usize) else { continue };
                if last.0 != b.page as i64 {
                    ctx.PSSetShaderResources(0, Some(&[Some(page.clone())]));
                    last.0 = b.page as i64;
                }
                if last.1 != b.blend as i64 {
                    ctx.OMSetBlendState(&gpu.blends[(b.blend as usize).min(3)], None, 0xFFFF_FFFF);
                    last.1 = b.blend as i64;
                }
                ctx.DrawIndexed(b.count, b.first, 0);
            }
            if resolve {
                let t = gpu.targets.get(&id).unwrap();
                ctx.ResolveSubresource(&t.resolved, 0, &t.msaa.as_ref().unwrap().0, 0, DXGI_FORMAT_R8G8B8A8_UNORM);
            }
            // Leave nothing bound that the picture pass could trip over (a page still bound as input, the target still bound as output).
            ctx.PSSetShaderResources(0, Some(&[None]));
            ctx.OMSetRenderTargets(None, None);
            ctx.IASetInputLayout(None);
            ctx.OMSetBlendState(None, None, 0xFFFF_FFFF);
        }
        let srv = gpu.targets.get(&id).unwrap().srv.clone();
        self.live.remove(&id);
        self.textures.insert(id, Texture { frames: vec![srv], delays: Vec::new(), frame: 0, width: size.0, height: size.1, mips: 1, bytes: size.0 as usize * size.1 as usize * 4, orientation: 1, video: false });
        Ok(())
    }

    /// Drop the target of picture `id` (see `forget`).
    pub(crate) fn spine_forget(&mut self, id: u64) {
        if let Some(s) = self.spine.as_mut() {
            s.targets.remove(&id);
        }
    }
}
