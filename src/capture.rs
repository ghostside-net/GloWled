//! Screen capture abstraction, Windows DXGI Desktop Duplication with GDI fallback,
//! and resilient error recovery (standby, resolution changes, UAC, fullscreen games).

/// Pixel layout of a captured frame buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    Rgb8,
    Bgra8,
    Rgba8,
    Gray8,
}

impl PixelFormat {
    pub fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Rgb8 => 3,
            Self::Bgra8 | Self::Rgba8 => 4,
            Self::Gray8 => 1,
        }
    }
}

/// Captured image buffer with dimensions and pixel format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameBuffer {
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    pub data: Vec<u8>,
}

impl FrameBuffer {
    pub fn new(width: u32, height: u32, format: PixelFormat, data: Vec<u8>) -> Self {
        Self {
            width,
            height,
            format,
            data,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0 || self.data.is_empty()
    }
}

/// Metadata describing an available display output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayInfo {
    /// 1-based monitor index matching `MonitorConfig::mss_index`.
    pub index: usize,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub left: i32,
    pub top: i32,
}

/// Result of a single screen capture attempt.
#[derive(Clone, Debug)]
pub enum CaptureStatus {
    /// A new frame was captured.
    NewFrame,
    /// Desktop content has not changed since the last frame (Eco Mode can idle).
    Unchanged,
    /// Display mode changed, monitor entered standby, UAC prompt appeared, or DXGI access was lost.
    DeviceLost(String),
    /// Non-fatal transient error; caller should back off and retry/reset.
    Error(String),
}

/// Unified trait for screen capture backends.
pub trait ScreenCapturer: Send {
    /// Captures the current screen frame directly into `out`, optionally downsampling to
    /// `(target_w, target_h)` when non-zero to avoid allocating multi-megabyte full-screen buffers.
    fn capture_into(
        &mut self,
        target_w: u32,
        target_h: u32,
        out: &mut FrameBuffer,
    ) -> CaptureStatus;

    /// Returns display metadata for the active output.
    fn display_info(&self) -> DisplayInfo;

    /// Re-initializes the capture session after resolution change, standby, or access loss.
    fn reset(&mut self) -> Result<(), String>;
}

/// Downsamples a raw mapped surface (`src_format`) directly into a compact `Rgb8` `FrameBuffer`
/// using fast bilinear-like grid sampling so 4K captures never allocate 33 MB heap buffers.
pub fn downsample_raw_into_rgb8(
    src: &[u8],
    src_stride: usize,
    src_w: u32,
    src_h: u32,
    src_format: PixelFormat,
    dst_w: u32,
    dst_h: u32,
    out: &mut FrameBuffer,
) {
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
        out.width = 0;
        out.height = 0;
        out.format = PixelFormat::Rgb8;
        out.data.clear();
        return;
    }

    let bpp = src_format.bytes_per_pixel();
    let needed = (dst_w as usize) * (dst_h as usize) * 3;
    out.width = dst_w;
    out.height = dst_h;
    out.format = PixelFormat::Rgb8;
    out.data.resize(needed, 0);

    // Fixed-point 16.16 step coordinates for exact, branch-minimal mapping
    let step_x = ((src_w as u64) << 16) / (dst_w as u64);
    let step_y = ((src_h as u64) << 16) / (dst_h as u64);
    let half_x = step_x >> 1;
    let half_y = step_y >> 1;

    for dy in 0..dst_h as usize {
        let sy = ((((dy as u64) * step_y + half_y) >> 16) as usize).min(src_h as usize - 1);
        let row_offset = sy * src_stride;
        if row_offset + (src_w as usize) * bpp > src.len() {
            continue;
        }
        let src_row = &src[row_offset..];
        let dst_row = &mut out.data[dy * (dst_w as usize) * 3..(dy + 1) * (dst_w as usize) * 3];

        for dx in 0..dst_w as usize {
            let sx = ((((dx as u64) * step_x + half_x) >> 16) as usize).min(src_w as usize - 1);
            let s_idx = sx * bpp;
            let d_idx = dx * 3;
            match src_format {
                PixelFormat::Bgra8 => {
                    dst_row[d_idx] = src_row[s_idx + 2];
                    dst_row[d_idx + 1] = src_row[s_idx + 1];
                    dst_row[d_idx + 2] = src_row[s_idx];
                }
                PixelFormat::Rgba8 | PixelFormat::Rgb8 => {
                    dst_row[d_idx] = src_row[s_idx];
                    dst_row[d_idx + 1] = src_row[s_idx + 1];
                    dst_row[d_idx + 2] = src_row[s_idx + 2];
                }
                PixelFormat::Gray8 => {
                    let g = src_row[s_idx];
                    dst_row[d_idx] = g;
                    dst_row[d_idx + 1] = g;
                    dst_row[d_idx + 2] = g;
                }
            }
        }
    }
}

/// Enumerates available displays (uses DXGI on Windows, sensible default on non-Windows).
pub fn enumerate_displays() -> Vec<DisplayInfo> {
    #[cfg(windows)]
    {
        let win_displays = win_backend::enumerate_windows_displays();
        if !win_displays.is_empty() {
            return win_displays;
        }
    }

    vec![DisplayInfo {
        index: 1,
        name: "Monitor 1 (Standard)".to_string(),
        width: 1920,
        height: 1080,
        left: 0,
        top: 0,
    }]
}

/// Creates the best available capturer for 1-based `monitor_index`.
pub fn create_capturer(monitor_index: usize) -> Box<dyn ScreenCapturer> {
    #[cfg(windows)]
    {
        if let Ok(dxgi) = win_backend::DxgiCapturer::new(monitor_index) {
            return Box::new(dxgi);
        }
        return Box::new(win_backend::GdiCapturer::new(monitor_index));
    }

    #[cfg(not(windows))]
    {
        Box::new(FallbackCapturer::new(monitor_index))
    }
}

/// Deterministic fallback / headless capturer for non-Windows environments and tests.
pub struct FallbackCapturer {
    info: DisplayInfo,
    frame_counter: u64,
    first_sent: bool,
    simulate_device_lost_every: Option<u64>,
}

impl FallbackCapturer {
    pub fn new(monitor_index: usize) -> Self {
        let idx = monitor_index.max(1);
        Self {
            info: DisplayInfo {
                index: idx,
                name: format!("Monitor {idx} (Fallback)"),
                width: 1920,
                height: 1080,
                left: 0,
                top: 0,
            },
            frame_counter: 0,
            first_sent: false,
            simulate_device_lost_every: None,
        }
    }

    pub fn with_simulated_faults(mut self, every_n_frames: u64) -> Self {
        self.simulate_device_lost_every = Some(every_n_frames.max(1));
        self
    }
}

impl ScreenCapturer for FallbackCapturer {
    fn capture_into(
        &mut self,
        target_w: u32,
        target_h: u32,
        out: &mut FrameBuffer,
    ) -> CaptureStatus {
        self.frame_counter = self.frame_counter.wrapping_add(1);
        if let Some(period) = self.simulate_device_lost_every {
            if self.frame_counter % period == 0 {
                self.first_sent = false;
                return CaptureStatus::DeviceLost("Simulated display reset / standby".to_string());
            }
        }

        if self.first_sent {
            // Static desktop simulation after initial frame
            return CaptureStatus::Unchanged;
        }
        self.first_sent = true;

        let w = if target_w > 0 { target_w } else { 128 };
        let h = if target_h > 0 { target_h } else { 72 };
        out.width = w;
        out.height = h;
        out.format = PixelFormat::Rgb8;
        out.data.clear();
        out.data.resize((w as usize) * (h as usize) * 3, 0);
        CaptureStatus::NewFrame
    }

    fn display_info(&self) -> DisplayInfo {
        self.info.clone()
    }

    fn reset(&mut self) -> Result<(), String> {
        self.first_sent = false;
        Ok(())
    }
}

#[cfg(windows)]
mod win_backend {
    use super::*;
    use std::ffi::c_void;
    use windows::core::Interface;
    use windows::Win32::Graphics::Direct3D::{
        D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
    };
    use windows::Win32::Graphics::Direct3D11::{
        D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D,
        D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE,
        D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
    };
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1, IDXGIOutput1, IDXGIOutputDuplication,
        IDXGIResource, DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_DEVICE_REMOVED,
        DXGI_ERROR_DEVICE_RESET, DXGI_ERROR_INVALID_CALL, DXGI_ERROR_WAIT_TIMEOUT,
        DXGI_OUTDUPL_FRAME_INFO,
    };
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
        SRCCOPY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };

    pub fn enumerate_windows_displays() -> Vec<DisplayInfo> {
        let mut displays = Vec::new();
        unsafe {
            let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
                return displays;
            };

            let mut adapter_idx = 0u32;
            let mut global_idx = 1usize;
            while let Ok(adapter) = factory.EnumAdapters1(adapter_idx) {
                let mut output_idx = 0u32;
                while let Ok(output) = adapter.EnumOutputs(output_idx) {
                    if let Ok(desc) = output.GetDesc() {
                        let rect = desc.DesktopCoordinates;
                        let width = (rect.right - rect.left).max(0) as u32;
                        let height = (rect.bottom - rect.top).max(0) as u32;
                        if width > 0 && height > 0 {
                            let raw_name = String::from_utf16_lossy(
                                &desc.DeviceName
                                    [..desc.DeviceName.iter().position(|&c| c == 0).unwrap_or(0)],
                            );
                            let name = if raw_name.is_empty() {
                                format!("Monitor {global_idx}")
                            } else {
                                format!("Monitor {global_idx} ({raw_name})")
                            };
                            displays.push(DisplayInfo {
                                index: global_idx,
                                name,
                                width,
                                height,
                                left: rect.left,
                                top: rect.top,
                            });
                            global_idx += 1;
                        }
                    }
                    output_idx += 1;
                }
                adapter_idx += 1;
            }
        }
        displays
    }

    pub struct DxgiCapturer {
        monitor_index: usize,
        info: DisplayInfo,
        device: Option<ID3D11Device>,
        context: Option<ID3D11DeviceContext>,
        duplication: Option<IDXGIOutputDuplication>,
        staging: Option<ID3D11Texture2D>,
        staging_w: u32,
        staging_h: u32,
        has_initial_frame: bool,
        gdi_fallback: Option<GdiCapturer>,
    }

    // COM objects created on the worker thread stay on the worker thread.
    unsafe impl Send for DxgiCapturer {}

    impl DxgiCapturer {
        pub fn new(monitor_index: usize) -> Result<Self, String> {
            let mut capturer = Self {
                monitor_index: monitor_index.max(1),
                info: DisplayInfo {
                    index: monitor_index.max(1),
                    name: format!("Monitor {}", monitor_index.max(1)),
                    width: 1920,
                    height: 1080,
                    left: 0,
                    top: 0,
                },
                device: None,
                context: None,
                duplication: None,
                staging: None,
                staging_w: 0,
                staging_h: 0,
                has_initial_frame: false,
                gdi_fallback: None,
            };
            capturer.init_duplication()?;
            Ok(capturer)
        }

        fn release_dxgi(&mut self) {
            self.duplication = None;
            self.staging = None;
            self.context = None;
            self.device = None;
            self.staging_w = 0;
            self.staging_h = 0;
            self.has_initial_frame = false;
        }

        fn init_duplication(&mut self) -> Result<(), String> {
            self.release_dxgi();
            unsafe {
                let factory: IDXGIFactory1 = CreateDXGIFactory1()
                    .map_err(|e| format!("CreateDXGIFactory1 failed: {e:?}"))?;

                let target_zero_based = self.monitor_index.saturating_sub(1);
                let mut current_idx = 0usize;
                let mut chosen: Option<(IDXGIAdapter1, IDXGIOutput1, DisplayInfo)> = None;

                let mut adapter_idx = 0u32;
                'outer: while let Ok(adapter) = factory.EnumAdapters1(adapter_idx) {
                    let mut output_idx = 0u32;
                    while let Ok(output) = adapter.EnumOutputs(output_idx) {
                        if let Ok(desc) = output.GetDesc() {
                            let rect = desc.DesktopCoordinates;
                            let w = (rect.right - rect.left).max(0) as u32;
                            let h = (rect.bottom - rect.top).max(0) as u32;
                            if w > 0 && h > 0 {
                                if current_idx == target_zero_based || chosen.is_none() {
                                    if let Ok(output1) = output.cast::<IDXGIOutput1>() {
                                        let info = DisplayInfo {
                                            index: self.monitor_index,
                                            name: format!("Monitor {}", self.monitor_index),
                                            width: w,
                                            height: h,
                                            left: rect.left,
                                            top: rect.top,
                                        };
                                        chosen = Some((adapter.clone(), output1, info));
                                        if current_idx == target_zero_based {
                                            break 'outer;
                                        }
                                    }
                                }
                                current_idx += 1;
                            }
                        }
                        output_idx += 1;
                    }
                    adapter_idx += 1;
                }

                let (adapter, output1, info) =
                    chosen.ok_or_else(|| "No active DXGI display output found".to_string())?;
                self.info = info;

                let feature_levels = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];
                let mut device = None;
                let mut context = None;
                D3D11CreateDevice(
                    &adapter,
                    D3D_DRIVER_TYPE_UNKNOWN,
                    None,
                    D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                    Some(&feature_levels),
                    D3D11_SDK_VERSION,
                    Some(&mut device),
                    None,
                    Some(&mut context),
                )
                .map_err(|e| format!("D3D11CreateDevice failed: {e:?}"))?;

                let device = device.ok_or_else(|| "D3D11 device was None".to_string())?;
                let context = context.ok_or_else(|| "D3D11 context was None".to_string())?;
                let duplication = output1
                    .DuplicateOutput(&device)
                    .map_err(|e| format!("DuplicateOutput failed: {e:?}"))?;

                self.device = Some(device);
                self.context = Some(context);
                self.duplication = Some(duplication);
                Ok(())
            }
        }

        fn ensure_staging_texture(&mut self, width: u32, height: u32) -> Result<(), String> {
            if self.staging.is_some() && self.staging_w == width && self.staging_h == height {
                return Ok(());
            }
            let device = self
                .device
                .as_ref()
                .ok_or_else(|| "Missing D3D11 device".to_string())?;
            let desc = D3D11_TEXTURE2D_DESC {
                Width: width,
                Height: height,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
            };
            unsafe {
                let mut tex = None;
                device
                    .CreateTexture2D(&desc, None, Some(&mut tex))
                    .map_err(|e| format!("CreateTexture2D staging failed: {e:?}"))?;
                self.staging = tex;
                self.staging_w = width;
                self.staging_h = height;
            }
            Ok(())
        }
    }

    impl ScreenCapturer for DxgiCapturer {
        fn capture_into(
            &mut self,
            target_w: u32,
            target_h: u32,
            out: &mut FrameBuffer,
        ) -> CaptureStatus {
            if let Some(gdi) = self.gdi_fallback.as_mut() {
                return gdi.capture_into(target_w, target_h, out);
            }

            let Some(dupl) = self.duplication.clone() else {
                return CaptureStatus::DeviceLost("DXGI duplication not initialized".to_string());
            };

            unsafe {
                let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
                let mut resource: Option<IDXGIResource> = None;

                // Non-blocking / 0ms poll so the worker loop maintains exact FPS pacing
                let acquire_res = dupl.AcquireNextFrame(0, &mut frame_info, &mut resource);
                if let Err(err) = acquire_res {
                    let code = err.code();
                    if code == DXGI_ERROR_WAIT_TIMEOUT {
                        return CaptureStatus::Unchanged;
                    }
                    if code == DXGI_ERROR_ACCESS_LOST
                        || code == DXGI_ERROR_INVALID_CALL
                        || code == DXGI_ERROR_DEVICE_REMOVED
                        || code == DXGI_ERROR_DEVICE_RESET
                    {
                        self.release_dxgi();
                        return CaptureStatus::DeviceLost(format!("DXGI transient loss: {err:?}"));
                    }
                    return CaptureStatus::Error(format!("AcquireNextFrame error: {err:?}"));
                }

                // Ensure ReleaseFrame is always called cleanly on exit
                struct FrameGuard(IDXGIOutputDuplication);
                impl Drop for FrameGuard {
                    fn drop(&mut self) {
                        unsafe {
                            let _ = self.0.ReleaseFrame();
                        }
                    }
                }
                let _guard = FrameGuard(dupl);

                if self.has_initial_frame
                    && frame_info.LastPresentTime == 0
                    && frame_info.AccumulatedFrames == 0
                {
                    return CaptureStatus::Unchanged;
                }

                let Some(res) = resource else {
                    return CaptureStatus::Unchanged;
                };
                let Ok(tex) = res.cast::<ID3D11Texture2D>() else {
                    return CaptureStatus::Error("Failed to cast DXGI resource to texture".into());
                };

                let mut tex_desc = D3D11_TEXTURE2D_DESC::default();
                tex.GetDesc(&mut tex_desc);
                let src_w = tex_desc.Width;
                let src_h = tex_desc.Height;
                if src_w == 0 || src_h == 0 {
                    return CaptureStatus::DeviceLost("Zero-sized desktop texture".into());
                }

                if let Err(e) = self.ensure_staging_texture(src_w, src_h) {
                    return CaptureStatus::Error(e);
                }
                let Some(context) = self.context.as_ref() else {
                    return CaptureStatus::DeviceLost("Missing D3D11 context".into());
                };
                let Some(staging) = self.staging.as_ref() else {
                    return CaptureStatus::DeviceLost("Missing staging texture".into());
                };

                let dst_res: ID3D11Resource = match staging.cast() {
                    Ok(r) => r,
                    Err(e) => return CaptureStatus::Error(format!("Cast staging failed: {e:?}")),
                };
                let src_res: ID3D11Resource = match tex.cast() {
                    Ok(r) => r,
                    Err(e) => return CaptureStatus::Error(format!("Cast src failed: {e:?}")),
                };

                context.CopyResource(&dst_res, &src_res);

                let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                if let Err(e) = context.Map(&dst_res, 0, D3D11_MAP_READ, 0, Some(&mut mapped)) {
                    return CaptureStatus::DeviceLost(format!("Map staging texture failed: {e:?}"));
                }

                let dst_w = if target_w > 0 { target_w } else { src_w };
                let dst_h = if target_h > 0 { target_h } else { src_h };
                let stride = mapped.RowPitch as usize;
                let total_bytes = stride.saturating_mul(src_h as usize);
                if !mapped.pData.is_null() && total_bytes > 0 {
                    let raw_slice =
                        std::slice::from_raw_parts(mapped.pData as *const u8, total_bytes);
                    downsample_raw_into_rgb8(
                        raw_slice,
                        stride,
                        src_w,
                        src_h,
                        PixelFormat::Bgra8,
                        dst_w,
                        dst_h,
                        out,
                    );
                    self.has_initial_frame = true;
                }

                context.Unmap(&dst_res, 0);
                self.info.width = src_w;
                self.info.height = src_h;
                CaptureStatus::NewFrame
            }
        }

        fn display_info(&self) -> DisplayInfo {
            if let Some(gdi) = self.gdi_fallback.as_ref() {
                return gdi.display_info();
            }
            self.info.clone()
        }

        fn reset(&mut self) -> Result<(), String> {
            match self.init_duplication() {
                Ok(()) => {
                    self.gdi_fallback = None;
                    Ok(())
                }
                Err(dxgi_err) => {
                    // Activate GDI fallback gracefully if DXGI duplication is blocked (e.g. UAC/RDP)
                    self.gdi_fallback = Some(GdiCapturer::new(self.monitor_index));
                    Err(dxgi_err)
                }
            }
        }
    }

    /// GDI BitBlt fallback capturer when DXGI Desktop Duplication is unavailable.
    pub struct GdiCapturer {
        info: DisplayInfo,
        scratch_bgra: Vec<u8>,
    }

    impl GdiCapturer {
        pub fn new(monitor_index: usize) -> Self {
            let displays = enumerate_windows_displays();
            let info = displays
                .into_iter()
                .find(|d| d.index == monitor_index)
                .unwrap_or_else(|| unsafe {
                    let w = GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1) as u32;
                    let h = GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1) as u32;
                    let left = GetSystemMetrics(SM_XVIRTUALSCREEN);
                    let top = GetSystemMetrics(SM_YVIRTUALSCREEN);
                    DisplayInfo {
                        index: monitor_index.max(1),
                        name: format!("Monitor {} (GDI)", monitor_index.max(1)),
                        width: w,
                        height: h,
                        left,
                        top,
                    }
                });
            Self {
                info,
                scratch_bgra: Vec::new(),
            }
        }
    }

    impl ScreenCapturer for GdiCapturer {
        fn capture_into(
            &mut self,
            target_w: u32,
            target_h: u32,
            out: &mut FrameBuffer,
        ) -> CaptureStatus {
            unsafe {
                let w = self.info.width as i32;
                let h = self.info.height as i32;
                if w <= 0 || h <= 0 {
                    return CaptureStatus::DeviceLost("Invalid GDI monitor bounds".into());
                }

                let hdc_screen = GetDC(None);
                if hdc_screen.is_invalid() {
                    return CaptureStatus::DeviceLost("GetDC(NULL) failed".into());
                }
                let hdc_mem = CreateCompatibleDC(hdc_screen);
                if hdc_mem.is_invalid() {
                    let _ = ReleaseDC(None, hdc_screen);
                    return CaptureStatus::DeviceLost("CreateCompatibleDC failed".into());
                }
                let hbm = CreateCompatibleBitmap(hdc_screen, w, h);
                if hbm.is_invalid() {
                    let _ = DeleteDC(hdc_mem);
                    let _ = ReleaseDC(None, hdc_screen);
                    return CaptureStatus::DeviceLost("CreateCompatibleBitmap failed".into());
                }

                let old_obj = SelectObject(hdc_mem, hbm);
                let ok = BitBlt(
                    hdc_mem,
                    0,
                    0,
                    w,
                    h,
                    hdc_screen,
                    self.info.left,
                    self.info.top,
                    SRCCOPY,
                )
                .is_ok();

                if ok {
                    let mut bmi = BITMAPINFO::default();
                    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
                    bmi.bmiHeader.biWidth = w;
                    bmi.bmiHeader.biHeight = -h; // Top-down DIB
                    bmi.bmiHeader.biPlanes = 1;
                    bmi.bmiHeader.biBitCount = 32;
                    bmi.bmiHeader.biCompression = BI_RGB.0;

                    let byte_len = (w as usize) * (h as usize) * 4;
                    self.scratch_bgra.resize(byte_len, 0);
                    let scanlines = GetDIBits(
                        hdc_mem,
                        hbm,
                        0,
                        h as u32,
                        Some(self.scratch_bgra.as_mut_ptr() as *mut c_void),
                        &mut bmi,
                        DIB_RGB_COLORS,
                    );
                    if scanlines > 0 {
                        let dst_w = if target_w > 0 { target_w } else { w as u32 };
                        let dst_h = if target_h > 0 { target_h } else { h as u32 };
                        downsample_raw_into_rgb8(
                            &self.scratch_bgra,
                            (w as usize) * 4,
                            w as u32,
                            h as u32,
                            PixelFormat::Bgra8,
                            dst_w,
                            dst_h,
                            out,
                        );
                    }
                }

                SelectObject(hdc_mem, old_obj);
                let _ = DeleteObject(hbm);
                let _ = DeleteDC(hdc_mem);
                let _ = ReleaseDC(None, hdc_screen);

                if ok {
                    CaptureStatus::NewFrame
                } else {
                    CaptureStatus::DeviceLost("BitBlt failed".into())
                }
            }
        }

        fn display_info(&self) -> DisplayInfo {
            self.info.clone()
        }

        fn reset(&mut self) -> Result<(), String> {
            *self = Self::new(self.info.index);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downsample_bgra8_drops_alpha_and_swaps_channels() {
        // 4x2 BGRA frame where B=10, G=20, R=30, A=255
        let mut src = Vec::with_capacity(4 * 2 * 4);
        for _ in 0..8 {
            src.extend_from_slice(&[10, 20, 30, 255]);
        }
        let mut out = FrameBuffer::new(0, 0, PixelFormat::Rgb8, Vec::new());
        downsample_raw_into_rgb8(&src, 16, 4, 2, PixelFormat::Bgra8, 2, 1, &mut out);
        assert_eq!(out.width, 2);
        assert_eq!(out.height, 1);
        assert_eq!(out.format, PixelFormat::Rgb8);
        assert_eq!(out.data, vec![30, 20, 10, 30, 20, 10]);
    }

    #[test]
    fn fallback_capturer_recovers_from_simulated_device_lost() {
        let mut cap = FallbackCapturer::new(1).with_simulated_faults(2);
        let mut frame = FrameBuffer::new(0, 0, PixelFormat::Rgb8, Vec::new());

        assert!(matches!(
            cap.capture_into(64, 36, &mut frame),
            CaptureStatus::NewFrame
        ));
        assert!(matches!(
            cap.capture_into(64, 36, &mut frame),
            CaptureStatus::DeviceLost(_)
        ));
        assert!(cap.reset().is_ok());
        assert!(matches!(
            cap.capture_into(64, 36, &mut frame),
            CaptureStatus::NewFrame
        ));
    }
}
