//! More things the renderer can draw: a picture blurred over the whole window (background), one chosen frame of an
//! animation (seek-bar preview), video frames that change many times a second, and the 360-degree view.

use super::*;

static PANO_PS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/pano_ps.cso"));

/// Pixel shader + sampler of the 360 view, created when the first panorama is drawn.
pub(crate) struct PanoPipeline {
    ps: ID3D11PixelShader,
    sampler: ID3D11SamplerState,
}

impl Renderer {
    /// The mip of `id` whose width is nearest to `target_px` (never finer than that), for `draw_blurred`.
    pub fn blur_lod(&self, id: u64, target_px: f32) -> Option<u32> {
        let t = self.textures.get(&id)?;
        let lod = (t.width.max(t.height) as f32 / target_px.max(1.0)).log2().floor().max(0.0) as u32;
        Some(lod.min(t.mips - 1))
    }

    /// Draw picture `id` into `dst` from mip `lod`: with the Catmull-Rom filter of the shader a tiny mip stretched over
    /// the window is a soft blur, for the price of one draw.
    pub fn draw_blurred(&mut self, id: u64, dst: [f32; 4], lod: u32) -> Result<(), String> {
        let Some((srv, tw, th, mips, orientation)) = self.textures.get(&id).map(|t| (t.srv().clone(), t.width, t.height, t.mips, t.orientation)) else { return Ok(()) };
        self.draw_quad(&srv, dst, [tw as f32, th as f32, lod.min(mips - 1) as f32, 0.0], [1.0; 4], [orientation as f32, 0.0, 0.0, 0.0])
    }

    /// Draw frame `frame` of an animation (not the one currently playing) into `dst`.
    pub fn draw_frame(&mut self, id: u64, frame: usize, dst: [f32; 4]) -> Result<(), String> {
        let Some((srv, tw, th, mips, ow)) = self.textures.get(&id).map(|t| (t.frames[frame.min(t.frames.len() - 1)].clone(), t.width, t.height, t.mips, t.oriented_size().0)) else { return Ok(()) };
        let scale = (dst[2] - dst[0]) / ow as f32;
        let lod = if scale < 1.0 { ((-scale.log2()).floor().max(0.0) as u32).min(mips - 1) } else { 0 };
        self.draw_quad(&srv, dst, [tw as f32, th as f32, lod as f32, 0.0], [1.0; 4], [0.0; 4])
    }

    /// Put a decoded video frame (BGRA, `w` x `h`) into texture `id`, creating it on the first call or when the size changes.
    /// The texture has a mip chain like every picture, so a 4K frame shrunk to a window stays smooth.
    pub fn upload_video(&mut self, id: u64, w: u32, h: u32, bgra: &[u8]) -> Result<(), String> {
        if bgra.len() < (w as usize) * (h as usize) * 4 || w == 0 || h == 0 || w > MAX_TEXTURE_DIM || h > MAX_TEXTURE_DIM {
            return Err("video frame has an unusable size".into());
        }
        unsafe {
            let reuse = self.textures.get(&id).filter(|t| t.video && t.width == w && t.height == h && t.frames.len() == 1).map(|t| t.frames[0].clone());
            let srv = match reuse {
                Some(s) => s,
                None => {
                    let mips = 32 - w.max(h).leading_zeros();
                    let td = D3D11_TEXTURE2D_DESC {
                        Width: w,
                        Height: h,
                        MipLevels: mips,
                        ArraySize: 1,
                        Format: DXGI_FORMAT_B8G8R8A8_UNORM_SRGB,
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
                    let srv: ID3D11ShaderResourceView = srv.ok_or("srv")?;
                    let bytes = w as usize * h as usize * 4 * 4 / 3;
                    self.textures.insert(id, Texture { frames: vec![srv.clone()], delays: Vec::new(), frame: 0, width: w, height: h, mips, bytes, orientation: 1, video: true });
                    srv
                }
            };
            let res = srv.GetResource().map_err(err("GetResource"))?;
            self.ctx.UpdateSubresource(&res, 0, None, bgra.as_ptr() as *const c_void, w * 4, 0);
            self.ctx.GenerateMips(&srv);
        }
        Ok(())
    }

    /// The 360 view: the whole window looks into equirectangular picture `id`. `yaw` and `pitch` in radians (pitch up is
    /// positive), `hfov` is the horizontal field of view in radians.
    pub fn draw_pano(&mut self, id: u64, yaw: f32, pitch: f32, hfov: f32) -> Result<(), String> {
        let Some((srv, tw)) = self.textures.get(&id).map(|t| (t.srv().clone(), t.width)) else { return Ok(()) };
        if self.pano.is_none() {
            unsafe {
                let mut ps = None;
                self.device.CreatePixelShader(PANO_PS, None, Some(&mut ps)).map_err(err("CreatePixelShader"))?;
                let sd = D3D11_SAMPLER_DESC {
                    Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                    AddressU: D3D11_TEXTURE_ADDRESS_WRAP,
                    AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                    MaxLOD: f32::MAX,
                    ComparisonFunc: D3D11_COMPARISON_NEVER,
                    ..Default::default()
                };
                let mut sampler = None;
                self.device.CreateSamplerState(&sd, Some(&mut sampler)).map_err(err("CreateSamplerState"))?;
                self.pano = Some(PanoPipeline { ps: ps.ok_or("ps")?, sampler: sampler.ok_or("sampler")? });
            }
        }
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        let half = (hfov / 2.0).tan();
        // Texels per screen pixel at the centre -> mip level.
        let lod = (tw as f32 * (hfov / w) / std::f32::consts::TAU).max(1.0).log2().max(0.0);
        let Some(pp) = self.pano.as_ref() else { return Ok(()) };
        let (ps, sampler) = (pp.ps.clone(), pp.sampler.clone());
        unsafe {
            self.ctx.PSSetShader(&ps, None);
            self.ctx.PSSetSamplers(0, Some(&[Some(sampler)]));
        }
        let r = self.draw_quad(&srv, [0.0, 0.0, w, h], [yaw, pitch, half, lod], [1.0; 4], [w / h, 0.0, 0.0, 0.0]);
        unsafe { self.ctx.PSSetShader(&self.ps, None) };
        r
    }
}
