//! High-performance edge extraction, black-bar detection, color correction,
//! Hermite smoothstep black-level cutoff, and temporal EMA smoothing.

use crate::capture::{FrameBuffer, PixelFormat};
use crate::config::{MonitorConfig, EDGE_BOTTOM, EDGE_LEFT, EDGE_RIGHT, EDGE_TOP};

/// Rectangular region of interest `(x, y, width, height)` in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Roi {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Edge bounding boxes for all four monitor edges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EdgeRois {
    pub left: Roi,
    pub top: Roi,
    pub right: Roi,
    pub bottom: Roi,
}

/// Detected symmetric letterbox / pillarbox black bar margins in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BlackBarInsets {
    pub top: u32,
    pub bottom: u32,
    pub left: u32,
    pub right: u32,
}

/// Precomputed 256-entry gamma lookup table to avoid per-pixel `powf` calls.
#[derive(Clone, Debug)]
pub struct GammaLut {
    gamma: f32,
    table: [u8; 256],
}

impl Default for GammaLut {
    fn default() -> Self {
        Self::new(1.0)
    }
}

impl GammaLut {
    pub fn new(gamma: f32) -> Self {
        let mut lut = Self {
            gamma: -1.0,
            table: [0; 256],
        };
        lut.update(gamma);
        lut
    }

    pub fn update(&mut self, gamma: f32) {
        let clamped = gamma.clamp(0.5, 4.0);
        if (self.gamma - clamped).abs() < 1e-4 {
            return;
        }
        self.gamma = clamped;
        if (clamped - 1.0).abs() < 1e-4 {
            for (i, slot) in self.table.iter_mut().enumerate() {
                *slot = i as u8;
            }
        } else {
            for (i, slot) in self.table.iter_mut().enumerate() {
                let norm = (i as f32) / 255.0;
                *slot = (norm.powf(clamped) * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }

    #[inline]
    pub fn map(&self, val: u8) -> u8 {
        self.table[val as usize]
    }

    #[inline]
    pub fn is_identity(&self) -> bool {
        (self.gamma - 1.0).abs() < 1e-4
    }
}

/// Reusable scratch buffers to keep per-frame allocations at zero in steady state.
#[derive(Clone, Debug, Default)]
pub struct ProcessingScratch {
    pub thumb_rgb: Vec<u8>,
    pub edge_pixels: Vec<[u8; 3]>,
    pub gamma_lut: GammaLut,
}

/// Temporal Exponential Moving Average (EMA) smoother with black-level snapping.
#[derive(Clone, Debug, Default)]
pub struct TemporalSmoother {
    prev_pixels: Vec<[u8; 3]>,
}

impl TemporalSmoother {
    pub fn reset(&mut self) {
        self.prev_pixels.clear();
    }

    /// Applies `smoothed = smoothing * prev + (1.0 - smoothing) * current` and snaps values
    /// below `black_threshold` cleanly to `(0, 0, 0)`.
    pub fn apply(&mut self, current_rgb_bytes: &mut [u8], smoothing: f32, black_threshold: f32) {
        let count = current_rgb_bytes.len() / 3;
        if count == 0 {
            return;
        }
        let s = smoothing.clamp(0.0, 0.95);
        if s > 0.0 && self.prev_pixels.len() == count {
            let one_minus_s = 1.0 - s;
            for (i, prev) in self.prev_pixels.iter().enumerate() {
                let base = i * 3;
                for c in 0..3 {
                    let blended = s * (prev[c] as f32)
                        + one_minus_s * (current_rgb_bytes[base + c] as f32);
                    current_rgb_bytes[base + c] = blended.clamp(0.0, 255.0) as u8;
                }
            }
            if black_threshold > 0.0 {
                apply_black_level_threshold_bytes(current_rgb_bytes, black_threshold, None);
            }
        }

        self.prev_pixels.resize(count, [0, 0, 0]);
        for (i, dst) in self.prev_pixels.iter_mut().enumerate() {
            let base = i * 3;
            *dst = [
                current_rgb_bytes[base],
                current_rgb_bytes[base + 1],
                current_rgb_bytes[base + 2],
            ];
        }
    }

    /// Seeds previous state for deterministic testing.
    pub fn seed(&mut self, pixels: Vec<[u8; 3]>) {
        self.prev_pixels = pixels;
    }
}

/// Resolves preset name (e.g. `"Auto (Adaptiv)"`, `"Balance (128x72)"`) to `(width, height)`.
pub fn resolve_capture_resolution(
    preset: &str,
    cfg: &MonitorConfig,
    oversample_factor: f32,
) -> (u32, u32) {
    match preset {
        "Auto (Adaptiv)" | "auto" | "Auto" | "" => {
            calculate_adaptive_resolution(cfg, oversample_factor, (32, 18), (1920, 1080), (128, 72))
        }
        "Performance (64x36)" => (64, 36),
        "Balance (128x72)" | "Balanced (128x72)" => (128, 72),
        "Quality (256x144)" => (256, 144),
        "Ultra (512x288)" => (512, 288),
        _ => (128, 72),
    }
}

/// Computes optimal downscaling resolution using the Nyquist-Shannon sampling theorem.
pub fn calculate_adaptive_resolution(
    cfg: &MonitorConfig,
    oversample_factor: f32,
    min_res: (u32, u32),
    max_res: (u32, u32),
    fallback: (u32, u32),
) -> (u32, u32) {
    let horizontal_leds = cfg.leds_top.max(cfg.leds_bottom);
    let vertical_leds = cfg.leds_left.max(cfg.leds_right);

    if horizontal_leds == 0
        || vertical_leds == 0
        || horizontal_leds > 10_000
        || vertical_leds > 10_000
    {
        return fallback;
    }

    let factor = oversample_factor.clamp(1.0, 8.0);
    let mut target_w = ((horizontal_leds as f32) * factor).round() as u32;
    let mut target_h = ((vertical_leds as f32) * factor).round() as u32;

    // Keep even dimensions for alignment
    target_w += target_w % 2;
    target_h += target_h % 2;

    let w = target_w.clamp(min_res.0, max_res.0);
    let h = target_h.clamp(min_res.1, max_res.1);
    (w, h)
}

/// Computes sampling depth in pixels along a dimension, supporting ratios `0.0..=1.0`
/// and absolute pixel distances `> 1.0`, clamped to `[min_pixels, dimension * max_ratio]`.
pub fn calculate_edge_depth_pixels(
    dimension: u32,
    depth: f32,
    max_ratio: f32,
    min_pixels: u32,
) -> u32 {
    if dimension == 0 {
        return min_pixels;
    }

    let pixels = if (0.0..=1.0).contains(&depth) {
        let clamped_ratio = depth.clamp(0.0, max_ratio);
        ((dimension as f32) * clamped_ratio).round() as u32
    } else if depth > 1.0 {
        depth.round() as u32
    } else {
        min_pixels
    };

    let max_allowed = min_pixels.max(((dimension as f32) * max_ratio) as u32);
    pixels.clamp(min_pixels, max_allowed)
}

/// Computes bounding boxes `(x, y, w, h)` for all four screen edges.
pub fn calculate_edge_rois(width: u32, height: u32, edge_depth: f32) -> EdgeRois {
    let px_left = calculate_edge_depth_pixels(width, edge_depth, 0.5, 1);
    let px_top = calculate_edge_depth_pixels(height, edge_depth, 0.5, 1);
    let px_right = calculate_edge_depth_pixels(width, edge_depth, 0.5, 1);
    let px_bottom = calculate_edge_depth_pixels(height, edge_depth, 0.5, 1);

    EdgeRois {
        left: Roi {
            x: 0,
            y: 0,
            w: px_left,
            h: height,
        },
        top: Roi {
            x: 0,
            y: 0,
            w: width,
            h: px_top,
        },
        right: Roi {
            x: width.saturating_sub(px_right),
            y: 0,
            w: px_right,
            h: height,
        },
        bottom: Roi {
            x: 0,
            y: height.saturating_sub(px_bottom),
            w: width,
            h: px_bottom,
        },
    }
}

/// Computes individual LED segment ROIs along an edge (`"left"`, `"top"`, `"right"`, `"bottom"`).
pub fn calculate_segment_rois(
    width: u32,
    height: u32,
    edge: &str,
    num_leds: usize,
    edge_depth: f32,
) -> Vec<Roi> {
    if num_leds == 0 || width == 0 || height == 0 {
        return Vec::new();
    }

    let depth_x = calculate_edge_depth_pixels(width, edge_depth, 0.5, 1);
    let depth_y = calculate_edge_depth_pixels(height, edge_depth, 0.5, 1);
    let mut rois = Vec::with_capacity(num_leds);

    match edge {
        EDGE_TOP | EDGE_BOTTOM => {
            let y = if edge == EDGE_TOP {
                0
            } else {
                height.saturating_sub(depth_y)
            };
            let h = depth_y;
            for i in 0..num_leds {
                let x1 = (((i as f64) * (width as f64)) / (num_leds as f64)).round() as u32;
                let x2 = ((((i + 1) as f64) * (width as f64)) / (num_leds as f64)).round() as u32;
                let seg_w = x2.saturating_sub(x1).max(1);
                rois.push(Roi { x: x1, y, w: seg_w, h });
            }
        }
        EDGE_LEFT | EDGE_RIGHT => {
            let x = if edge == EDGE_LEFT {
                0
            } else {
                width.saturating_sub(depth_x)
            };
            let w = depth_x;
            for i in 0..num_leds {
                let y1 = (((i as f64) * (height as f64)) / (num_leds as f64)).round() as u32;
                let y2 = ((((i + 1) as f64) * (height as f64)) / (num_leds as f64)).round() as u32;
                let seg_h = y2.saturating_sub(y1).max(1);
                rois.push(Roi { x, y: y1, w, h: seg_h });
            }
        }
        _ => {}
    }

    rois
}

/// Detects symmetric letterbox (top/bottom) and pillarbox (left/right) black bars up to 25% of
/// each dimension. Returns `(0, 0, 0, 0)` if the entire frame is black or bars are asymmetric.
pub fn detect_black_bars(rgb: &[u8], width: u32, height: u32, threshold: u8) -> BlackBarInsets {
    let w = width as usize;
    let h = height as usize;
    if w < 4 || h < 4 || rgb.len() < w * h * 3 {
        return BlackBarInsets::default();
    }

    let is_row_black = |y: usize| -> bool {
        let row = &rgb[y * w * 3..(y + 1) * w * 3];
        row.chunks_exact(3)
            .all(|p| p[0].max(p[1]).max(p[2]) <= threshold)
    };

    let is_col_black = |x: usize, y_start: usize, y_end: usize| -> bool {
        for y in y_start..y_end {
            let idx = (y * w + x) * 3;
            let max_c = rgb[idx].max(rgb[idx + 1]).max(rgb[idx + 2]);
            if max_c > threshold {
                return false;
            }
        }
        true
    };

    let max_v = h / 4;
    let mut top_bar = 0usize;
    while top_bar < max_v && is_row_black(top_bar) {
        top_bar += 1;
    }

    let mut bottom_bar = 0usize;
    while bottom_bar < max_v && is_row_black(h - 1 - bottom_bar) {
        bottom_bar += 1;
    }

    // Require roughly symmetric top & bottom bars AND non-black inner content
    let v_tol = (h / 20).max(2);
    let mut eff_top = 0u32;
    let mut eff_bottom = 0u32;
    if top_bar > 0 && bottom_bar > 0 && top_bar.abs_diff(bottom_bar) <= v_tol {
        // Confirm center contains non-black content so a dark frame isn't shifted unnecessarily
        let mid_y = h / 2;
        if !is_row_black(mid_y) {
            eff_top = top_bar as u32;
            eff_bottom = bottom_bar as u32;
        }
    }

    let y_start = eff_top as usize;
    let y_end = h.saturating_sub(eff_bottom as usize);
    let max_h = w / 4;
    let mut left_bar = 0usize;
    while left_bar < max_h && is_col_black(left_bar, y_start, y_end) {
        left_bar += 1;
    }

    let mut right_bar = 0usize;
    while right_bar < max_h && is_col_black(w - 1 - right_bar, y_start, y_end) {
        right_bar += 1;
    }

    let h_tol = (w / 20).max(2);
    let mut eff_left = 0u32;
    let mut eff_right = 0u32;
    if left_bar > 0 && right_bar > 0 && left_bar.abs_diff(right_bar) <= h_tol {
        let mid_x = w / 2;
        if !is_col_black(mid_x, y_start, y_end) {
            eff_left = left_bar as u32;
            eff_right = right_bar as u32;
        }
    }

    BlackBarInsets {
        top: eff_top,
        bottom: eff_bottom,
        left: eff_left,
        right: eff_right,
    }
}

/// Applies black level cutoff with Hermite smoothstep fade-out (`3*t^2 - 2*t^3`)
/// on max channel intensity `max(R, G, B)`.
pub fn apply_black_level_threshold(
    pixels: &mut [[u8; 3]],
    threshold: f32,
    transition_range: Option<f32>,
) {
    if pixels.is_empty() {
        return;
    }
    let thresh = if threshold > 0.0 && threshold <= 1.0 {
        threshold * 255.0
    } else {
        threshold
    }
    .clamp(0.0, 255.0);

    if thresh <= 0.0 {
        return;
    }

    let trans_w = match transition_range {
        None => thresh.clamp(4.0, 30.0),
        Some(r) => {
            let scaled = if r > 0.0 && r <= 1.0 { r * 255.0 } else { r };
            scaled.max(1.0)
        }
    };
    let knee = (thresh + trans_w).min(255.0);
    let denom = (knee - thresh).max(1e-5);

    for px in pixels.iter_mut() {
        let intensity = px[0].max(px[1]).max(px[2]) as f32;
        if intensity <= thresh {
            *px = [0, 0, 0];
        } else if intensity < knee {
            let t = (intensity - thresh) / denom;
            let factor = t * t * (3.0 - 2.0 * t);
            px[0] = ((px[0] as f32) * factor).round().clamp(0.0, 255.0) as u8;
            px[1] = ((px[1] as f32) * factor).round().clamp(0.0, 255.0) as u8;
            px[2] = ((px[2] as f32) * factor).round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// Convenience helper operating on flat RGB byte slices `[R, G, B, ...]`.
pub fn apply_black_level_threshold_bytes(
    rgb_bytes: &mut [u8],
    threshold: f32,
    transition_range: Option<f32>,
) {
    let thresh = if threshold > 0.0 && threshold <= 1.0 {
        threshold * 255.0
    } else {
        threshold
    }
    .clamp(0.0, 255.0);
    if thresh <= 0.0 || rgb_bytes.len() < 3 {
        return;
    }

    let trans_w = match transition_range {
        None => thresh.clamp(4.0, 30.0),
        Some(r) => {
            let scaled = if r > 0.0 && r <= 1.0 { r * 255.0 } else { r };
            scaled.max(1.0)
        }
    };
    let knee = (thresh + trans_w).min(255.0);
    let denom = (knee - thresh).max(1e-5);

    for px in rgb_bytes.chunks_exact_mut(3) {
        let intensity = px[0].max(px[1]).max(px[2]) as f32;
        if intensity <= thresh {
            px[0] = 0;
            px[1] = 0;
            px[2] = 0;
        } else if intensity < knee {
            let t = (intensity - thresh) / denom;
            let factor = t * t * (3.0 - 2.0 * t);
            px[0] = ((px[0] as f32) * factor).round().clamp(0.0, 255.0) as u8;
            px[1] = ((px[1] as f32) * factor).round().clamp(0.0, 255.0) as u8;
            px[2] = ((px[2] as f32) * factor).round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// Adjusts saturation in HSV space without allocating intermediate buffers.
#[inline]
pub fn adjust_saturation_rgb(r: u8, g: u8, b: u8, saturation: f32) -> [u8; 3] {
    if (saturation - 1.0).abs() < 1e-4 {
        return [r, g, b];
    }
    let rf = r as f32 / 255.0;
    let gf = g as f32 / 255.0;
    let bf = b as f32 / 255.0;

    let max_c = rf.max(gf).max(bf);
    let min_c = rf.min(gf).min(bf);
    let delta = max_c - min_c;

    if delta < 1e-6 || max_c < 1e-6 {
        return [r, g, b];
    }

    let sat = ((delta / max_c) * saturation).clamp(0.0, 1.0);
    if sat <= 1e-6 {
        let v = (max_c * 255.0).round().clamp(0.0, 255.0) as u8;
        return [v, v, v];
    }

    let mut hue = if (max_c - rf).abs() < 1e-6 {
        (gf - bf) / delta
    } else if (max_c - gf).abs() < 1e-6 {
        2.0 + (bf - rf) / delta
    } else {
        4.0 + (rf - gf) / delta
    };
    if hue < 0.0 {
        hue += 6.0;
    }

    let sector = hue.floor() as i32;
    let frac = hue - (sector as f32);
    let v = max_c;
    let p = v * (1.0 - sat);
    let q = v * (1.0 - sat * frac);
    let t = v * (1.0 - sat * (1.0 - frac));

    let (or, og, ob) = match sector % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };

    [
        (or * 255.0).round().clamp(0.0, 255.0) as u8,
        (og * 255.0).round().clamp(0.0, 255.0) as u8,
        (ob * 255.0).round().clamp(0.0, 255.0) as u8,
    ]
}

/// Downscales `frame` into `scratch.thumb_rgb` (`dst_w x dst_h` RGB8), safely sanitizing
/// BGRA/RGBA alpha channels and grayscale inputs, and applies saturation, brightness,
/// white balance, and gamma LUT in a single fused pass.
#[allow(clippy::too_many_arguments)]
fn downscale_and_correct_into_thumb(
    frame: &FrameBuffer,
    dst_w: u32,
    dst_h: u32,
    brightness: f32,
    saturation: f32,
    white_balance: [f32; 3],
    gamma: f32,
    scratch: &mut ProcessingScratch,
) {
    let dw = dst_w.max(1) as usize;
    let dh = dst_h.max(1) as usize;
    scratch.thumb_rgb.resize(dw * dh * 3, 0);
    scratch.gamma_lut.update(gamma);

    if frame.is_empty() {
        scratch.thumb_rgb.fill(0);
        return;
    }

    let sw = frame.width as usize;
    let sh = frame.height as usize;
    let bpp = frame.format.bytes_per_pixel();
    let expected_len = sw * sh * bpp;
    if frame.data.len() < expected_len {
        scratch.thumb_rgb.fill(0);
        return;
    }

    let apply_sat = (saturation - 1.0).abs() > 1e-4;
    let apply_scale = (brightness - 1.0).abs() > 1e-4
        || (white_balance[0] - 1.0).abs() > 1e-4
        || (white_balance[1] - 1.0).abs() > 1e-4
        || (white_balance[2] - 1.0).abs() > 1e-4;
    let apply_gamma = !scratch.gamma_lut.is_identity();

    let scale_r = brightness * white_balance[0];
    let scale_g = brightness * white_balance[1];
    let scale_b = brightness * white_balance[2];

    // Use box-area averaging when downscaling significantly, or bilinear-centered sampling
    let step_x = ((sw as u64) << 16) / (dw as u64);
    let step_y = ((sh as u64) << 16) / (dh as u64);
    let half_x = step_x >> 1;
    let half_y = step_y >> 1;

    for dy in 0..dh {
        let sy = ((((dy as u64) * step_y + half_y) >> 16) as usize).min(sh - 1);
        let row_base = sy * sw * bpp;
        let dst_row = &mut scratch.thumb_rgb[dy * dw * 3..(dy + 1) * dw * 3];

        for dx in 0..dw {
            let sx = ((((dx as u64) * step_x + half_x) >> 16) as usize).min(sw - 1);
            let s_idx = row_base + sx * bpp;
            let [mut r, mut g, mut b] = match frame.format {
                PixelFormat::Rgb8 | PixelFormat::Rgba8 => [
                    frame.data[s_idx],
                    frame.data[s_idx + 1],
                    frame.data[s_idx + 2],
                ],
                PixelFormat::Bgra8 => [
                    frame.data[s_idx + 2],
                    frame.data[s_idx + 1],
                    frame.data[s_idx],
                ],
                PixelFormat::Gray8 => {
                    let v = frame.data[s_idx];
                    [v, v, v]
                }
            };

            if apply_sat {
                [r, g, b] = adjust_saturation_rgb(r, g, b, saturation);
            }
            if apply_scale {
                r = ((r as f32) * scale_r).round().clamp(0.0, 255.0) as u8;
                g = ((g as f32) * scale_g).round().clamp(0.0, 255.0) as u8;
                b = ((b as f32) * scale_b).round().clamp(0.0, 255.0) as u8;
            }
            if apply_gamma {
                r = scratch.gamma_lut.map(r);
                g = scratch.gamma_lut.map(g);
                b = scratch.gamma_lut.map(b);
            }

            let d_idx = dx * 3;
            dst_row[d_idx] = r;
            dst_row[d_idx + 1] = g;
            dst_row[d_idx + 2] = b;
        }
    }
}

/// Computes area-averaged RGB values for `num_leds` along a single edge from `thumb_rgb`.
fn sample_edge_area_averages(
    thumb_rgb: &[u8],
    w: u32,
    h: u32,
    edge: &str,
    num_leds: usize,
    edge_depth: f32,
    bars: BlackBarInsets,
    out_pixels: &mut Vec<[u8; 3]>,
) {
    out_pixels.clear();
    if num_leds == 0 || w == 0 || h == 0 {
        return;
    }

    let active_x0 = bars.left.min(w.saturating_sub(1));
    let active_x1 = w.saturating_sub(bars.right).max(active_x0 + 1);
    let active_y0 = bars.top.min(h.saturating_sub(1));
    let active_y1 = h.saturating_sub(bars.bottom).max(active_y0 + 1);
    let active_w = active_x1 - active_x0;
    let active_h = active_y1 - active_y0;

    let depth_x = calculate_edge_depth_pixels(active_w, edge_depth, 0.5, 1);
    let depth_y = calculate_edge_depth_pixels(active_h, edge_depth, 0.5, 1);

    let stride_w = w as usize;

    for i in 0..num_leds {
        let (rx0, ry0, rx1, ry1) = match edge {
            EDGE_TOP => {
                let x1 = active_x0 + (((i as f64) * (active_w as f64)) / (num_leds as f64)).round() as u32;
                let x2 = active_x0 + ((((i + 1) as f64) * (active_w as f64)) / (num_leds as f64)).round() as u32;
                (x1, active_y0, x2.max(x1 + 1).min(active_x1), (active_y0 + depth_y).min(active_y1))
            }
            EDGE_BOTTOM => {
                let x1 = active_x0 + (((i as f64) * (active_w as f64)) / (num_leds as f64)).round() as u32;
                let x2 = active_x0 + ((((i + 1) as f64) * (active_w as f64)) / (num_leds as f64)).round() as u32;
                let y0 = active_y1.saturating_sub(depth_y).max(active_y0);
                (x1, y0, x2.max(x1 + 1).min(active_x1), active_y1)
            }
            EDGE_LEFT => {
                let y1 = active_y0 + (((i as f64) * (active_h as f64)) / (num_leds as f64)).round() as u32;
                let y2 = active_y0 + ((((i + 1) as f64) * (active_h as f64)) / (num_leds as f64)).round() as u32;
                (active_x0, y1, (active_x0 + depth_x).min(active_x1), y2.max(y1 + 1).min(active_y1))
            }
            EDGE_RIGHT => {
                let y1 = active_y0 + (((i as f64) * (active_h as f64)) / (num_leds as f64)).round() as u32;
                let y2 = active_y0 + ((((i + 1) as f64) * (active_h as f64)) / (num_leds as f64)).round() as u32;
                let x0 = active_x1.saturating_sub(depth_x).max(active_x0);
                (x0, y1, active_x1, y2.max(y1 + 1).min(active_y1))
            }
            _ => continue,
        };

        let mut sum_r: u64 = 0;
        let mut sum_g: u64 = 0;
        let mut sum_b: u64 = 0;
        let mut count: u64 = 0;

        for y in ry0..ry1 {
            let row_start = (y as usize) * stride_w;
            for x in rx0..rx1 {
                let idx = (row_start + (x as usize)) * 3;
                sum_r += thumb_rgb[idx] as u64;
                sum_g += thumb_rgb[idx + 1] as u64;
                sum_b += thumb_rgb[idx + 2] as u64;
                count += 1;
            }
        }

        if count > 0 {
            out_pixels.push([
                (sum_r / count) as u8,
                (sum_g / count) as u8,
                (sum_b / count) as u8,
            ]);
        } else {
            out_pixels.push([0, 0, 0]);
        }
    }
}

/// Processes a single monitor frame into contiguous RGB bytes ordered according to
/// `cfg.edge_order` and `cfg.edge_directions`.
#[allow(clippy::too_many_arguments)]
pub fn process_monitor_frame_into(
    frame: &FrameBuffer,
    cfg: &MonitorConfig,
    resolution_preset: &str,
    brightness: f32,
    saturation: f32,
    white_balance: [f32; 3],
    gamma: f32,
    edge_depth: f32,
    black_threshold: f32,
    black_bar_detection: bool,
    oversample_factor: f32,
    scratch: &mut ProcessingScratch,
    out_rgb_bytes: &mut Vec<u8>,
) {
    let (eff_w, eff_h) = resolve_capture_resolution(resolution_preset, cfg, oversample_factor);

    downscale_and_correct_into_thumb(
        frame,
        eff_w,
        eff_h,
        brightness,
        saturation,
        white_balance,
        gamma,
        scratch,
    );

    let bars = if black_bar_detection {
        let thresh_u8 = if black_threshold > 0.0 && black_threshold <= 1.0 {
            (black_threshold * 255.0).round() as u8
        } else {
            black_threshold.round().clamp(0.0, 255.0) as u8
        };
        detect_black_bars(&scratch.thumb_rgb, eff_w, eff_h, thresh_u8)
    } else {
        BlackBarInsets::default()
    };

    for edge_name in cfg.effective_edge_order() {
        let num_leds = cfg.led_count_for_edge(edge_name);
        if num_leds == 0 {
            continue;
        }
        sample_edge_area_averages(
            &scratch.thumb_rgb,
            eff_w,
            eff_h,
            edge_name,
            num_leds,
            edge_depth,
            bars,
            &mut scratch.edge_pixels,
        );

        if black_threshold > 0.0 {
            apply_black_level_threshold(&mut scratch.edge_pixels, black_threshold, None);
        }

        if cfg.is_edge_reversed(edge_name) {
            scratch.edge_pixels.reverse();
        }

        for px in &scratch.edge_pixels {
            out_rgb_bytes.extend_from_slice(px);
        }
    }
}

/// Convenience allocating wrapper around `process_monitor_frame_into`.
#[allow(clippy::too_many_arguments)]
pub fn process_monitor_frame(
    frame: &FrameBuffer,
    cfg: &MonitorConfig,
    resolution_preset: &str,
    brightness: f32,
    saturation: f32,
    white_balance: [f32; 3],
    gamma: f32,
    edge_depth: f32,
    black_threshold: f32,
    black_bar_detection: bool,
    oversample_factor: f32,
) -> Vec<u8> {
    let mut scratch = ProcessingScratch::default();
    let mut out = Vec::with_capacity(cfg.total_leds() * 3);
    process_monitor_frame_into(
        frame,
        cfg,
        resolution_preset,
        brightness,
        saturation,
        white_balance,
        gamma,
        edge_depth,
        black_threshold,
        black_bar_detection,
        oversample_factor,
        &mut scratch,
        &mut out,
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_cfg() -> MonitorConfig {
        MonitorConfig::default()
    }

    #[test]
    fn adaptive_resolution_nyquist_and_edge_cases() {
        let mut cfg = sample_cfg();
        cfg.leds_top = 30;
        cfg.leds_bottom = 30;
        cfg.leds_left = 18;
        cfg.leds_right = 18;
        assert_eq!(
            calculate_adaptive_resolution(&cfg, 2.0, (32, 18), (1920, 1080), (128, 72)),
            (60, 36)
        );
        assert_eq!(
            calculate_adaptive_resolution(&cfg, 3.0, (32, 18), (1920, 1080), (128, 72)),
            (90, 54)
        );

        let zero_cfg = MonitorConfig {
            leds_left: 0,
            leds_top: 0,
            leds_right: 0,
            leds_bottom: 0,
            ..Default::default()
        };
        assert_eq!(
            calculate_adaptive_resolution(&zero_cfg, 2.0, (32, 18), (1920, 1080), (128, 72)),
            (128, 72)
        );

        let outlier_cfg = MonitorConfig {
            leds_top: 15000,
            leds_left: 9000,
            ..Default::default()
        };
        assert_eq!(
            calculate_adaptive_resolution(&outlier_cfg, 2.0, (32, 18), (1920, 1080), (128, 72)),
            (128, 72)
        );
    }

    #[test]
    fn edge_depth_and_rois_clamping() {
        assert_eq!(calculate_edge_depth_pixels(100, 0.0, 0.5, 1), 1);
        assert_eq!(calculate_edge_depth_pixels(100, 0.05, 0.5, 1), 5);
        assert_eq!(calculate_edge_depth_pixels(100, 0.50, 0.5, 1), 50);
        assert_eq!(calculate_edge_depth_pixels(100, 0.70, 0.5, 1), 50);
        assert_eq!(calculate_edge_depth_pixels(100, 15.0, 0.5, 1), 15);
        assert_eq!(calculate_edge_depth_pixels(100, 85.0, 0.5, 1), 50);

        let rois = calculate_edge_rois(1920, 1080, 0.10);
        assert_eq!(
            rois.left,
            Roi {
                x: 0,
                y: 0,
                w: 192,
                h: 1080
            }
        );
        assert_eq!(
            rois.bottom,
            Roi {
                x: 0,
                y: 972,
                w: 1920,
                h: 108
            }
        );

        let segs = calculate_segment_rois(100, 50, EDGE_TOP, 5, 0.10);
        assert_eq!(segs.len(), 5);
        assert_eq!(
            segs[0],
            Roi {
                x: 0,
                y: 0,
                w: 20,
                h: 5
            }
        );
        assert_eq!(
            segs[4],
            Roi {
                x: 80,
                y: 0,
                w: 20,
                h: 5
            }
        );
    }

    #[test]
    fn black_level_smoothstep_and_pure_colors() {
        let mut pixels = vec![
            [5, 5, 5],
            [15, 15, 15],
            [20, 20, 20],
            [30, 30, 30],
            [100, 100, 100],
            [0, 0, 255], // Pure blue must NOT be suppressed
        ];
        apply_black_level_threshold(&mut pixels, 15.0, Some(15.0));
        assert_eq!(pixels[0], [0, 0, 0]);
        assert_eq!(pixels[1], [0, 0, 0]);
        assert!(pixels[2][0] > 0 && pixels[2][0] < 20);
        assert_eq!(pixels[3], [30, 30, 30]);
        assert_eq!(pixels[4], [100, 100, 100]);
        assert_eq!(pixels[5], [0, 0, 255]);
    }

    #[test]
    fn bgra_alpha_sanitization_and_dark_gray_cutoff() {
        let cfg = sample_cfg();
        // 4-channel BGRA pure black with alpha = 255
        let mut bgra = Vec::with_capacity(100 * 100 * 4);
        for _ in 0..100 * 100 {
            bgra.extend_from_slice(&[0, 0, 0, 255]);
        }
        let fb = FrameBuffer::new(100, 100, PixelFormat::Bgra8, bgra);
        let out = process_monitor_frame(
            &fb,
            &cfg,
            "Performance (64x36)",
            1.0,
            1.0,
            [1.0, 1.0, 1.0],
            1.0,
            0.08,
            15.0,
            true,
            2.0,
        );
        assert_eq!(out.len(), cfg.total_leds() * 3);
        assert!(out.iter().all(|&b| b == 0));

        // Dark gray #080808 below threshold 15
        let dark_rgb = vec![8u8; 100 * 100 * 3];
        let fb_dark = FrameBuffer::new(100, 100, PixelFormat::Rgb8, dark_rgb);
        let out_dark = process_monitor_frame(
            &fb_dark,
            &cfg,
            "Performance (64x36)",
            1.0,
            1.0,
            [1.0, 1.0, 1.0],
            1.0,
            0.08,
            15.0,
            true,
            2.0,
        );
        assert!(out_dark.iter().all(|&b| b == 0));
    }

    #[test]
    fn black_bar_detection_letterbox_skips_cinema_bars() {
        let w = 64u32;
        let h = 36u32;
        let mut data = vec![0u8; (w * h * 3) as usize];
        // Top 6 rows and bottom 6 rows are black; inner rows (6..30) are bright red
        for y in 6..30 {
            for x in 0..w {
                let idx = ((y * w + x) * 3) as usize;
                data[idx] = 240;
                data[idx + 1] = 20;
                data[idx + 2] = 10;
            }
        }
        let bars = detect_black_bars(&data, w, h, 15);
        assert_eq!(bars.top, 6);
        assert_eq!(bars.bottom, 6);
        assert_eq!(bars.left, 0);
        assert_eq!(bars.right, 0);

        let fb = FrameBuffer::new(w, h, PixelFormat::Rgb8, data);
        let mut cfg = sample_cfg();
        cfg.edge_order = vec![EDGE_TOP.to_string()];
        let out = process_monitor_frame(
            &fb,
            &cfg,
            "Performance (64x36)",
            1.0,
            1.0,
            [1.0, 1.0, 1.0],
            1.0,
            0.08,
            15.0,
            true,
            2.0,
        );
        assert_eq!(out.len(), cfg.leds_top * 3);
        // Should sample the red inner content instead of the black letterbox bar
        assert!(out.chunks_exact(3).all(|px| px[0] >= 230 && px[1] <= 25));
    }

    #[test]
    fn gamma_white_balance_brightness_and_saturation() {
        let cfg = MonitorConfig {
            edge_order: vec![EDGE_TOP.to_string()],
            leds_top: 4,
            ..Default::default()
        };
        let mut rgb = Vec::with_capacity(64 * 36 * 3);
        for _ in 0..(64 * 36) {
            rgb.extend_from_slice(&[128, 128, 128]);
        }
        let fb = FrameBuffer::new(64, 36, PixelFormat::Rgb8, rgb);

        let gamma_out = process_monitor_frame(
            &fb,
            &cfg,
            "Performance (64x36)",
            1.0,
            1.0,
            [1.0, 0.5, 1.0],
            2.0,
            0.08,
            0.0,
            false,
            2.0,
        );
        // R ~ (128/255)^2 * 255 ≈ 64, G ~ (64/255)^2 * 255 ≈ 16
        assert!((gamma_out[0] as i32 - 64).abs() <= 2);
        assert!((gamma_out[1] as i32 - 16).abs() <= 2);

        // Desaturation of pure red -> gray
        let mut red_buf = Vec::with_capacity(64 * 36 * 3);
        for _ in 0..(64 * 36) {
            red_buf.extend_from_slice(&[200, 0, 0]);
        }
        let fb_red = FrameBuffer::new(64, 36, PixelFormat::Rgb8, red_buf);
        let desat = process_monitor_frame(
            &fb_red,
            &cfg,
            "Performance (64x36)",
            1.0,
            0.0,
            [1.0, 1.0, 1.0],
            1.0,
            0.08,
            0.0,
            false,
            2.0,
        );
        assert_eq!(desat[0], desat[1]);
        assert_eq!(desat[1], desat[2]);
    }

    #[test]
    fn edge_direction_inversion_reverses_segments() {
        let mut data = vec![0u8; 100 * 100 * 3];
        for y in 0..20 {
            for x in 0..100 {
                let v = ((x as f32 / 99.0) * 255.0).round() as u8;
                let idx = (y * 100 + x) * 3;
                data[idx] = v;
                data[idx + 1] = v;
                data[idx + 2] = v;
            }
        }
        let fb = FrameBuffer::new(100, 100, PixelFormat::Rgb8, data);

        let mut cfg_norm = sample_cfg();
        cfg_norm.edge_order = vec![EDGE_TOP.to_string()];
        cfg_norm.edge_directions.insert(EDGE_TOP.to_string(), 1);

        let mut cfg_inv = cfg_norm.clone();
        cfg_inv.edge_directions.insert(EDGE_TOP.to_string(), -1);

        let out_norm = process_monitor_frame(
            &fb,
            &cfg_norm,
            "Balance (128x72)",
            1.0,
            1.0,
            [1.0, 1.0, 1.0],
            1.0,
            0.08,
            0.0,
            false,
            2.0,
        );
        let out_inv = process_monitor_frame(
            &fb,
            &cfg_inv,
            "Balance (128x72)",
            1.0,
            1.0,
            [1.0, 1.0, 1.0],
            1.0,
            0.08,
            0.0,
            false,
            2.0,
        );

        let norm_chunks: Vec<[u8; 3]> = out_norm.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
        let mut inv_chunks: Vec<[u8; 3]> = out_inv.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
        inv_chunks.reverse();
        assert_eq!(norm_chunks, inv_chunks);
    }

    #[test]
    fn temporal_smoother_ema_and_black_snapping() {
        let mut smoother = TemporalSmoother::default();
        smoother.seed(vec![[20, 20, 20]; 4]);
        let mut incoming = vec![0u8; 12];
        // 0.5 * 20 + 0.5 * 0 = 10 -> snapped to 0 because <= threshold 15
        smoother.apply(&mut incoming, 0.5, 15.0);
        assert!(incoming.iter().all(|&b| b == 0));

        // Without black threshold: 0.5 * 0 + 0.5 * 255 = 127
        smoother.seed(vec![[0, 0, 0]; 4]);
        let mut white = vec![255u8; 12];
        smoother.apply(&mut white, 0.5, 0.0);
        assert!(white.iter().all(|&b| (b as i32 - 127).abs() <= 1));
    }
}
