//! Late binding: the few Media Foundation and winmm entry points are looked up with `GetProcAddress` the first time they
//! are needed. A normal import would make Windows load `mfplat.dll`, `mfreadwrite.dll` and `winmm.dll` into every start
//! of every program that links this crate, media or not.

use std::ffi::c_void;
use std::sync::OnceLock;
use windows::core::{Interface, HRESULT, PCSTR, PCWSTR};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Media::MediaFoundation::{IMFAttributes, IMFMediaType, IMFSourceReader};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

type StartupFn = unsafe extern "system" fn(u32, u32) -> HRESULT;
type CreateAttrFn = unsafe extern "system" fn(*mut *mut c_void, u32) -> HRESULT;
type CreateTypeFn = unsafe extern "system" fn(*mut *mut c_void) -> HRESULT;
type CreateReaderFn = unsafe extern "system" fn(PCWSTR, *mut c_void, *mut *mut c_void) -> HRESULT;
type CreateDxgiManagerFn = unsafe extern "system" fn(*mut u32, *mut *mut c_void) -> HRESULT;

pub struct Mf {
    create_attributes: CreateAttrFn,
    create_media_type: CreateTypeFn,
    create_reader: CreateReaderFn,
    create_dxgi_manager: CreateDxgiManagerFn,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn lib(name: &str) -> Option<HMODULE> {
    let w = wide(name);
    LoadLibraryW(PCWSTR(w.as_ptr())).ok()
}

unsafe fn sym<T: Copy>(m: HMODULE, name: &str) -> Option<T> {
    let c = format!("{name}\0");
    GetProcAddress(m, PCSTR(c.as_ptr())).map(|f| std::mem::transmute_copy::<_, T>(&f))
}

static MF: OnceLock<Option<Mf>> = OnceLock::new();

/// Media Foundation, started once for the process. None when this Windows does not have it (N editions without the pack).
pub fn mf() -> Result<&'static Mf, String> {
    MF.get_or_init(|| unsafe {
        let plat = lib("mfplat.dll")?;
        let rw = lib("mfreadwrite.dll")?;
        let startup: StartupFn = sym(plat, "MFStartup")?;
        if startup(windows::Win32::Media::MediaFoundation::MF_VERSION, 0).is_err() {
            return None;
        }
        Some(Mf { create_attributes: sym(plat, "MFCreateAttributes")?, create_media_type: sym(plat, "MFCreateMediaType")?, create_reader: sym(rw, "MFCreateSourceReaderFromURL")?, create_dxgi_manager: sym(plat, "MFCreateDXGIDeviceManager")? })
    })
    .as_ref()
    .ok_or_else(|| "Media Foundation is not available on this PC".to_string())
}

/// Every thread that calls into Media Foundation needs COM first.
pub fn init_thread() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

impl Mf {
    pub fn attributes(&self) -> Result<IMFAttributes, String> {
        let mut p = std::ptr::null_mut();
        unsafe {
            (self.create_attributes)(&mut p, 4).ok().map_err(|e| e.to_string())?;
            Ok(IMFAttributes::from_raw(p))
        }
    }

    pub fn media_type(&self) -> Result<IMFMediaType, String> {
        let mut p = std::ptr::null_mut();
        unsafe {
            (self.create_media_type)(&mut p).ok().map_err(|e| e.to_string())?;
            Ok(IMFMediaType::from_raw(p))
        }
    }

    /// A source reader for `path` with software colour conversion switched on.
    pub fn reader(&self, path: &std::path::Path) -> Result<IMFSourceReader, String> {
        self.reader_with(path, false)
    }

    /// `gpu`: decode (and convert to RGB) on the graphics card through a Direct3D 11 device handed to the reader.
    pub fn reader_with(&self, path: &std::path::Path, gpu: bool) -> Result<IMFSourceReader, String> {
        use windows::Win32::Media::MediaFoundation::{MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, MF_SOURCE_READER_D3D_MANAGER, MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING};
        let attrs = self.attributes()?;
        let w = wide(&path.to_string_lossy());
        unsafe {
            if let (true, Some(m)) = (gpu, self.dxgi_manager()) {
                attrs.SetUnknown(&MF_SOURCE_READER_D3D_MANAGER, &m).map_err(|e| e.to_string())?;
                attrs.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1).map_err(|e| e.to_string())?;
                attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1).map_err(|e| e.to_string())?;
            } else {
                attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1).map_err(|e| e.to_string())?;
            }
            let mut p = std::ptr::null_mut();
            (self.create_reader)(PCWSTR(w.as_ptr()), attrs.as_raw(), &mut p).ok().map_err(|e| format!("this file can't be played here ({e})"))?;
            Ok(IMFSourceReader::from_raw(p))
        }
    }
}

// ---- a Direct3D 11 device for hardware decoding -----------------------------------------------------------------------

type D3d11CreateFn = unsafe extern "system" fn(*mut c_void, i32, *mut c_void, u32, *const i32, u32, u32, *mut *mut c_void, *mut i32, *mut *mut c_void) -> HRESULT;

struct Gpu {
    manager: windows::Win32::Media::MediaFoundation::IMFDXGIDeviceManager,
}
unsafe impl Send for Gpu {}
unsafe impl Sync for Gpu {}

static GPU: OnceLock<Option<Gpu>> = OnceLock::new();

impl Mf {
    /// The DXGI device manager of one shared D3D11 device (None when there is no usable graphics decoder path).
    fn dxgi_manager(&self) -> Option<windows::Win32::Media::MediaFoundation::IMFDXGIDeviceManager> {
        use windows::Win32::Graphics::Direct3D10::ID3D10Multithread;
        use windows::Win32::Graphics::Direct3D11::ID3D11Device;
        GPU.get_or_init(|| unsafe {
            let d3d = lib("d3d11.dll")?;
            let create: D3d11CreateFn = sym(d3d, "D3D11CreateDevice")?;
            let (mut dev, mut ctx, mut level) = (std::ptr::null_mut(), std::ptr::null_mut(), 0i32);
            // hardware driver, BGRA + video support
            create(std::ptr::null_mut(), 1, std::ptr::null_mut(), 0x20 | 0x800, std::ptr::null(), 0, 7, &mut dev, &mut level, &mut ctx).ok().ok()?;
            let device = ID3D11Device::from_raw(dev);
            if !ctx.is_null() {
                drop(windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext::from_raw(ctx));
            }
            if let Ok(mt) = device.cast::<ID3D10Multithread>() {
                let _ = mt.SetMultithreadProtected(true);
            }
            let (mut token, mut p) = (0u32, std::ptr::null_mut());
            (self.create_dxgi_manager)(&mut token, &mut p).ok().ok()?;
            let manager = windows::Win32::Media::MediaFoundation::IMFDXGIDeviceManager::from_raw(p);
            manager.ResetDevice(&device, token).ok()?;
            Some(Gpu { manager })
        })
        .as_ref()
        .map(|g| g.manager.clone())
    }
}

// ---- winmm (waveOut) -------------------------------------------------------------------------------------------------

#[repr(C)]
pub struct WaveFormat {
    pub tag: u16,
    pub channels: u16,
    pub rate: u32,
    pub bytes_per_sec: u32,
    pub block_align: u16,
    pub bits: u16,
    pub extra: u16,
}

#[repr(C)]
pub struct WaveHdr {
    pub data: *mut u8,
    pub len: u32,
    pub recorded: u32,
    pub user: usize,
    pub flags: u32,
    pub loops: u32,
    pub next: *mut WaveHdr,
    pub reserved: usize,
}

pub const WHDR_DONE: u32 = 1;
pub const CALLBACK_NULL: u32 = 0;
pub const WAVE_MAPPER: u32 = 0xFFFF_FFFF;

pub struct WinMm {
    pub open: unsafe extern "system" fn(*mut isize, u32, *const WaveFormat, usize, usize, u32) -> u32,
    pub close: unsafe extern "system" fn(isize) -> u32,
    pub prepare: unsafe extern "system" fn(isize, *mut WaveHdr, u32) -> u32,
    pub unprepare: unsafe extern "system" fn(isize, *mut WaveHdr, u32) -> u32,
    pub write: unsafe extern "system" fn(isize, *mut WaveHdr, u32) -> u32,
    pub reset: unsafe extern "system" fn(isize) -> u32,
    pub pause: unsafe extern "system" fn(isize) -> u32,
    pub restart: unsafe extern "system" fn(isize) -> u32,
    /// `MMTIME` is a 16-byte union here: `[0..4]` = the kind asked for (2 = samples), `[4..8]` = the sample count.
    pub get_position: unsafe extern "system" fn(isize, *mut [u32; 4], u32) -> u32,
}

static WINMM: OnceLock<Option<WinMm>> = OnceLock::new();

pub fn winmm() -> Option<&'static WinMm> {
    WINMM
        .get_or_init(|| unsafe {
            let m = lib("winmm.dll")?;
            Some(WinMm {
                open: sym(m, "waveOutOpen")?,
                close: sym(m, "waveOutClose")?,
                prepare: sym(m, "waveOutPrepareHeader")?,
                unprepare: sym(m, "waveOutUnprepareHeader")?,
                write: sym(m, "waveOutWrite")?,
                reset: sym(m, "waveOutReset")?,
                pause: sym(m, "waveOutPause")?,
                restart: sym(m, "waveOutRestart")?,
                get_position: sym(m, "waveOutGetPosition")?,
            })
        })
        .as_ref()
}
