//! 3D model drawing: indexed triangle meshes with a depth buffer, lit by a single headlight.
//! Created lazily (the first model opened), so ordinary image viewing pays nothing for it.

use nv_codecs::model::{self, Mesh, ModelStyle, Orbit, Shading};
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::Arc;
use windows::core::PCSTR;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;

static MESH_VS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/mesh_vs.cso"));
static MESH_PS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/mesh_ps.cso"));

/// One run of triangles with its own surface.
struct GpuPart {
    first: u32,
    count: u32,
    texture: Option<usize>,
    normal_map: Option<usize>,
    color: [f32; 3],
    alpha_cut: bool,
}

pub(crate) struct GpuMesh {
    vertices: ID3D11Buffer,
    indices: ID3D11Buffer,
    /// What to draw: the model's parts, or one plain part covering everything.
    parts: Vec<GpuPart>,
    textures: Vec<ID3D11ShaderResourceView>,
    /// The vertices carry colours that the shader multiplies in.
    vertex_colors: bool,
    /// Kept for its bounds and up axis (the camera needs them).
    source: Arc<Mesh>,
}

/// Bytes per vertex: position, normal, texture coordinates, colour.
const VERTEX_STRIDE: u32 = 36;

pub(crate) struct MeshPipeline {
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    layout: ID3D11InputLayout,
    cbuf: ID3D11Buffer,
    rasterizer: ID3D11RasterizerState,
    /// Solid fill pushed away from the viewer, so wireframe lines drawn at the true depth stay visible on top of it.
    rasterizer_back: ID3D11RasterizerState,
    rasterizer_wire: ID3D11RasterizerState,
    sampler: ID3D11SamplerState,
    depth_state: ID3D11DepthStencilState,
    /// Depth test without writing, and a stencil that lets each pixel through once (overlapping shadow triangles don't double up).
    depth_shadow: ID3D11DepthStencilState,
    blend_shadow: ID3D11BlendState,
    depth: Option<(ID3D11DepthStencilView, (u32, u32))>,
    pub(crate) meshes: HashMap<u64, GpuMesh>,
}

fn err<T: std::fmt::Display>(what: &'static str) -> impl FnOnce(T) -> String {
    move |e| format!("{what}: {e}")
}

impl MeshPipeline {
    pub(crate) fn new(device: &ID3D11Device) -> Result<Self, String> {
        unsafe {
            let mut vs = None;
            device.CreateVertexShader(MESH_VS, None, Some(&mut vs)).map_err(err("mesh vertex shader"))?;
            let mut ps = None;
            device.CreatePixelShader(MESH_PS, None, Some(&mut ps)).map_err(err("mesh pixel shader"))?;
            let elements = [
                D3D11_INPUT_ELEMENT_DESC {
                    SemanticName: PCSTR(b"POSITION\0".as_ptr()),
                    SemanticIndex: 0,
                    Format: DXGI_FORMAT_R32G32B32_FLOAT,
                    InputSlot: 0,
                    AlignedByteOffset: 0,
                    InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                    InstanceDataStepRate: 0,
                },
                D3D11_INPUT_ELEMENT_DESC {
                    SemanticName: PCSTR(b"NORMAL\0".as_ptr()),
                    SemanticIndex: 0,
                    Format: DXGI_FORMAT_R32G32B32_FLOAT,
                    InputSlot: 0,
                    AlignedByteOffset: 12,
                    InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                    InstanceDataStepRate: 0,
                },
                D3D11_INPUT_ELEMENT_DESC {
                    SemanticName: PCSTR(b"TEXCOORD\0".as_ptr()),
                    SemanticIndex: 0,
                    Format: DXGI_FORMAT_R32G32_FLOAT,
                    InputSlot: 0,
                    AlignedByteOffset: 24,
                    InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                    InstanceDataStepRate: 0,
                },
                D3D11_INPUT_ELEMENT_DESC {
                    SemanticName: PCSTR(b"COLOR\0".as_ptr()),
                    SemanticIndex: 0,
                    Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                    InputSlot: 0,
                    AlignedByteOffset: 32,
                    InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                    InstanceDataStepRate: 0,
                },
            ];
            let mut layout = None;
            device.CreateInputLayout(&elements, MESH_VS, Some(&mut layout)).map_err(err("mesh input layout"))?;
            let cb = D3D11_BUFFER_DESC {
                ByteWidth: 192,
                Usage: D3D11_USAGE_DYNAMIC,
                BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                ..Default::default()
            };
            let mut cbuf = None;
            device.CreateBuffer(&cb, None, Some(&mut cbuf)).map_err(err("mesh constants"))?;
            let rd = D3D11_RASTERIZER_DESC { FillMode: D3D11_FILL_SOLID, CullMode: D3D11_CULL_NONE, DepthClipEnable: true.into(), ..Default::default() };
            let mut rasterizer = None;
            device.CreateRasterizerState(&rd, Some(&mut rasterizer)).map_err(err("mesh rasterizer"))?;
            let rd_back = D3D11_RASTERIZER_DESC { DepthBias: 6000, SlopeScaledDepthBias: 2.0, ..rd };
            let mut rasterizer_back = None;
            device.CreateRasterizerState(&rd_back, Some(&mut rasterizer_back)).map_err(err("mesh rasterizer"))?;
            let rd_wire = D3D11_RASTERIZER_DESC { FillMode: D3D11_FILL_WIREFRAME, ..rd };
            let mut rasterizer_wire = None;
            device.CreateRasterizerState(&rd_wire, Some(&mut rasterizer_wire)).map_err(err("mesh rasterizer"))?;
            let stencil_once = D3D11_DEPTH_STENCILOP_DESC {
                StencilFailOp: D3D11_STENCIL_OP_KEEP,
                StencilDepthFailOp: D3D11_STENCIL_OP_KEEP,
                StencilPassOp: D3D11_STENCIL_OP_INCR_SAT,
                StencilFunc: D3D11_COMPARISON_EQUAL,
            };
            let ds_shadow = D3D11_DEPTH_STENCIL_DESC {
                DepthEnable: true.into(),
                DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ZERO,
                DepthFunc: D3D11_COMPARISON_LESS,
                StencilEnable: true.into(),
                StencilReadMask: 0xFF,
                StencilWriteMask: 0xFF,
                FrontFace: stencil_once,
                BackFace: stencil_once,
            };
            let mut depth_shadow = None;
            device.CreateDepthStencilState(&ds_shadow, Some(&mut depth_shadow)).map_err(err("mesh shadow depth state"))?;
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
            let mut blend_shadow = None;
            device.CreateBlendState(&bd, Some(&mut blend_shadow)).map_err(err("mesh shadow blend"))?;
            let dd = D3D11_DEPTH_STENCIL_DESC {
                DepthEnable: true.into(),
                DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ALL,
                DepthFunc: D3D11_COMPARISON_LESS,
                ..Default::default()
            };
            let sd = D3D11_SAMPLER_DESC {
                Filter: D3D11_FILTER_ANISOTROPIC,
                AddressU: D3D11_TEXTURE_ADDRESS_WRAP,
                AddressV: D3D11_TEXTURE_ADDRESS_WRAP,
                AddressW: D3D11_TEXTURE_ADDRESS_WRAP,
                MaxAnisotropy: 8,
                ComparisonFunc: D3D11_COMPARISON_NEVER,
                MinLOD: 0.0,
                MaxLOD: f32::MAX,
                ..Default::default()
            };
            let mut sampler = None;
            device.CreateSamplerState(&sd, Some(&mut sampler)).map_err(err("mesh sampler"))?;
            let mut depth_state = None;
            device.CreateDepthStencilState(&dd, Some(&mut depth_state)).map_err(err("mesh depth state"))?;
            Ok(MeshPipeline {
                vs: vs.ok_or("vs")?,
                ps: ps.ok_or("ps")?,
                layout: layout.ok_or("layout")?,
                cbuf: cbuf.ok_or("cbuf")?,
                rasterizer: rasterizer.ok_or("rs")?,
                rasterizer_back: rasterizer_back.ok_or("rs")?,
                rasterizer_wire: rasterizer_wire.ok_or("rs")?,
                sampler: sampler.ok_or("sampler")?,
                depth_state: depth_state.ok_or("dss")?,
                depth_shadow: depth_shadow.ok_or("dss")?,
                blend_shadow: blend_shadow.ok_or("blend")?,
                depth: None,
                meshes: HashMap::new(),
            })
        }
    }

    pub(crate) fn upload(&mut self, device: &ID3D11Device, ctx: &ID3D11DeviceContext, id: u64, mesh: &Arc<Mesh>) -> Result<(), String> {
        let n = mesh.positions.len();
        let (has_uv, has_col) = (mesh.uvs.len() == n, mesh.colors.len() == n);
        let mut verts: Vec<u8> = Vec::with_capacity(n * VERTEX_STRIDE as usize);
        for i in 0..n {
            for f in mesh.positions[i].iter().chain(mesh.normals[i].iter()) {
                verts.extend_from_slice(&f.to_le_bytes());
            }
            let uv = if has_uv { mesh.uvs[i] } else { [0.0, 0.0] };
            for f in uv {
                verts.extend_from_slice(&f.to_le_bytes());
            }
            verts.extend_from_slice(&if has_col { mesh.colors[i] } else { [255, 255, 255, 255] });
        }
        let make = |bytes: &[u8], bind: D3D11_BIND_FLAG, what: &'static str| -> Result<ID3D11Buffer, String> {
            let desc = D3D11_BUFFER_DESC { ByteWidth: bytes.len() as u32, Usage: D3D11_USAGE_IMMUTABLE, BindFlags: bind.0 as u32, ..Default::default() };
            let init = D3D11_SUBRESOURCE_DATA { pSysMem: bytes.as_ptr() as *const c_void, SysMemPitch: 0, SysMemSlicePitch: 0 };
            let mut b = None;
            unsafe { device.CreateBuffer(&desc, Some(&init), Some(&mut b)).map_err(err(what))? };
            b.ok_or_else(|| what.to_string())
        };
        let ibytes = unsafe { std::slice::from_raw_parts(mesh.indices.as_ptr() as *const u8, mesh.indices.len() * 4) };
        if verts.len() > 1_800_000_000 {
            return Err("model too large for the GPU".into());
        }
        let vertices = make(&verts, D3D11_BIND_VERTEX_BUFFER, "vertex buffer")?;
        let indices = make(ibytes, D3D11_BIND_INDEX_BUFFER, "index buffer")?;

        let mut textures = Vec::new();
        for t in &mesh.textures {
            textures.push(upload_texture(device, ctx, t)?);
        }
        let parts: Vec<GpuPart> = if mesh.parts.is_empty() {
            // One plain part. Models with vertex colours show them as they are, others get the grey surface.
            let color = if has_col { [1.0; 3] } else { model::BASE_COLOR };
            vec![GpuPart { first: 0, count: mesh.indices.len() as u32, texture: None, normal_map: None, color, alpha_cut: false }]
        } else {
            mesh.parts.iter().map(|p| GpuPart { first: p.first, count: p.count, texture: p.texture.filter(|&t| t < textures.len() && has_uv), normal_map: p.normal_map.filter(|&t| t < textures.len() && has_uv), color: p.color, alpha_cut: p.alpha_cut }).collect()
        };
        self.meshes.insert(id, GpuMesh { vertices, indices, parts, textures, vertex_colors: has_col, source: mesh.clone() });
        Ok(())
    }

    fn depth_view(&mut self, device: &ID3D11Device, size: (u32, u32)) -> Result<ID3D11DepthStencilView, String> {
        if let Some((dsv, s)) = &self.depth {
            if *s == size {
                return Ok(dsv.clone());
            }
        }
        unsafe {
            let td = D3D11_TEXTURE2D_DESC {
                Width: size.0,
                Height: size.1,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_D24_UNORM_S8_UINT,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_DEPTH_STENCIL.0 as u32,
                ..Default::default()
            };
            let mut tex = None;
            device.CreateTexture2D(&td, None, Some(&mut tex)).map_err(err("depth texture"))?;
            let tex: ID3D11Texture2D = tex.ok_or("depth texture")?;
            let mut dsv = None;
            device.CreateDepthStencilView(&tex, None, Some(&mut dsv)).map_err(err("depth view"))?;
            let dsv: ID3D11DepthStencilView = dsv.ok_or("depth view")?;
            self.depth = Some((dsv.clone(), size));
            Ok(dsv)
        }
    }

    /// Draw mesh `id` seen through `orbit` and the user's `style` into `rtv`. The caller restores its own 2D state afterwards.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw(
        &mut self,
        device: &ID3D11Device,
        ctx: &ID3D11DeviceContext,
        rtv: &ID3D11RenderTargetView,
        size: (u32, u32),
        id: u64,
        orbit: &Orbit,
        style: &ModelStyle,
    ) -> Result<bool, String> {
        if !self.meshes.contains_key(&id) {
            return Ok(false);
        }
        let dsv = self.depth_view(device, size)?;
        let gm = &self.meshes[&id];
        let cam = model::camera_styled(&gm.source, orbit, size.0 as f32 / size.1.max(1) as f32, style);
        let look = match style.shading {
            Shading::Combined => 0.0,
            Shading::Diffuse => 1.0,
            Shading::Cartoon => 2.0,
            Shading::Normals => 3.0,
            Shading::Unlit => 4.0,
        };
        unsafe {
            ctx.OMSetRenderTargets(Some(&[Some(rtv.clone())]), Some(&dsv));
            ctx.ClearDepthStencilView(&dsv, (D3D11_CLEAR_DEPTH.0 | D3D11_CLEAR_STENCIL.0) as u32, 1.0, 0);
            ctx.IASetInputLayout(&self.layout);
            ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            let (stride, offset) = (VERTEX_STRIDE, 0u32);
            ctx.IASetVertexBuffers(0, 1, Some(&Some(gm.vertices.clone())), Some(&stride), Some(&offset));
            ctx.IASetIndexBuffer(&gm.indices, DXGI_FORMAT_R32_UINT, 0);
            ctx.VSSetShader(&self.vs, None);
            ctx.VSSetConstantBuffers(0, Some(&[Some(self.cbuf.clone())]));
            ctx.PSSetShader(&self.ps, None);
            ctx.PSSetConstantBuffers(0, Some(&[Some(self.cbuf.clone())]));
            ctx.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));

            // 1. The floor shadow: the model flattened along the light, translucent black, each pixel darkened once.
            if let Some(sm) = &cam.shadow_mvp {
                ctx.OMSetDepthStencilState(&self.depth_shadow, 0);
                ctx.OMSetBlendState(&self.blend_shadow, None, 0xFFFF_FFFF);
                ctx.RSSetState(&self.rasterizer);
                self.draw_parts(ctx, gm, &cam, sm, style, look, 1.0)?;
            }
            // 2. The model itself. With the wireframe on it is drawn dimmed and pushed back, then the lines go on top.
            ctx.OMSetDepthStencilState(&self.depth_state, 0);
            ctx.OMSetBlendState(None, None, 0xFFFF_FFFF);
            ctx.RSSetState(if style.wireframe { &self.rasterizer_back } else { &self.rasterizer });
            self.draw_parts(ctx, gm, &cam, &cam.mvp, style, look, if style.wireframe { 3.0 } else { 0.0 })?;
            if style.wireframe {
                ctx.RSSetState(&self.rasterizer_wire);
                self.draw_parts(ctx, gm, &cam, &cam.mvp, style, look, 2.0)?;
            }
            ctx.PSSetShaderResources(0, Some(&[None, None]));
        }
        Ok(true)
    }

    /// Draw every part of `gm` once with `matrix` as the model -> clip transform. `pass` is the shader's pass number.
    #[allow(clippy::too_many_arguments)]
    unsafe fn draw_parts(&self, ctx: &ID3D11DeviceContext, gm: &GpuMesh, cam: &model::Camera, matrix: &model::Mat4, style: &ModelStyle, look: f32, pass: f32) -> Result<(), String> {
        for part in &gm.parts {
            // "Texture off" also switches off vertex colours: the surface is the plain grey (or the part's own colour for untextured parts).
            let coloured = part.texture.is_some() || gm.vertex_colors;
            let plain = !style.texture && coloured;
            let use_tex = part.texture.is_some() && style.texture;
            let use_vcol = gm.vertex_colors && style.texture;
            let color = if plain { model::BASE_COLOR } else { part.color };
            let use_normal_map = part.normal_map.is_some() && style.normal_map;
            let mut m = D3D11_MAPPED_SUBRESOURCE::default();
            ctx.Map(&self.cbuf, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut m)).map_err(err("Map"))?;
            let p = m.pData as *mut f32;
            let mut floats = Vec::with_capacity(48);
            floats.extend(matrix.iter().flatten());
            floats.extend(cam.view_rot.iter().flatten());
            floats.extend([cam.light_view[0], cam.light_view[1], cam.light_view[2], 0.0]);
            floats.extend([color[0], color[1], color[2], 1.0]);
            floats.extend([
                if use_tex { 1.0 } else { 0.0 },
                if use_vcol { 1.0 } else { 0.0 },
                if part.alpha_cut && use_tex { 1.0 } else { 0.0 },
                if use_normal_map { 1.0 } else { 0.0 },
            ]);
            floats.extend([look, pass, 0.0, 0.0]);
            std::ptr::copy_nonoverlapping(floats.as_ptr(), p, floats.len());
            ctx.Unmap(&self.cbuf, 0);
            let srv = if use_tex { part.texture.and_then(|t| gm.textures.get(t)).cloned() } else { None };
            let nsrv = if use_normal_map { part.normal_map.and_then(|t| gm.textures.get(t)).cloned() } else { None };
            ctx.PSSetShaderResources(0, Some(&[srv, nsrv]));
            ctx.DrawIndexed(part.count, part.first, 0);
        }
        Ok(())
    }
}

/// A model texture as a shader resource with a full mip chain (sRGB numbers, sampled without gamma conversion).
fn upload_texture(device: &ID3D11Device, ctx: &ID3D11DeviceContext, t: &model::ModelTexture) -> Result<ID3D11ShaderResourceView, String> {
    unsafe {
        let mips = 32 - t.width.max(t.height).leading_zeros();
        let td = D3D11_TEXTURE2D_DESC {
            Width: t.width,
            Height: t.height,
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
        device.CreateTexture2D(&td, None, Some(&mut tex)).map_err(err("model texture"))?;
        let tex: ID3D11Texture2D = tex.ok_or("model texture")?;
        ctx.UpdateSubresource(&tex, 0, None, t.rgba.as_ptr() as *const c_void, t.width * 4, 0);
        let mut srv = None;
        device.CreateShaderResourceView(&tex, None, Some(&mut srv)).map_err(err("model texture view"))?;
        let srv: ID3D11ShaderResourceView = srv.ok_or("model texture view")?;
        ctx.GenerateMips(&srv);
        Ok(srv)
    }
}
