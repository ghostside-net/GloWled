"""
worker.py - Ultra-lightweight, high-performance WLED Ambilight Capture Worker.
Optimized for minimal CPU and memory footprint with zero-copy SIMD processing,
adaptive capture resolution, configurable sampling depth, and black level handling.
"""

from __future__ import annotations

import socket
import sys
import threading
import time
from typing import Any

import cv2
import numpy as np

# Resolution lookup table with adaptive and fixed presets
RESOLUTION_PRESETS: dict[str, tuple[int, int]] = {
    "Auto (Adaptiv)": (0, 0),
    "Performance (64x36)": (64, 36),
    "Balance (128x72)": (128, 72),
    "Balanced (128x72)": (128, 72),
    "Quality (256x144)": (256, 144),
    "Ultra (512x288)": (512, 288),
}

# Optional high-performance DirectX capture for Windows
_HAS_DXCAM = False
dxcam: Any = None
try:
    if sys.platform == "win32":
        import dxcam
        _HAS_DXCAM = True
except Exception:
    _HAS_DXCAM = False

# Cross-platform fallback screen capture
try:
    import mss
    _HAS_MSS = True
except Exception:
    _HAS_MSS = False


def calculate_adaptive_resolution(
    cfg: dict[str, Any] | None = None,
    oversample_factor: float = 2.0,
    min_res: tuple[int, int] = (32, 18),
    max_res: tuple[int, int] = (1920, 1080),
    fallback: tuple[int, int] = (128, 72),
) -> tuple[int, int]:
    """Calculate an optimal, resource-efficient capture and downscaling resolution.

    Applies the Nyquist-Shannon sampling theorem based on the number of LEDs
    per edge to prevent aliasing artifacts while minimizing CPU and GPU overhead.

    Args:
        cfg: Monitor configuration dict containing 'leds_left', 'leds_top',
            'leds_right', and 'leds_bottom'.
        oversample_factor: Oversampling multiplier along each axis (default 2.0x,
            typical 2.0x to 4.0x).
        min_res: Minimum allowed resolution (width, height).
        max_res: Maximum allowed resolution (width, height) to avoid excessive load.
        fallback: Safe fallback resolution when LED counts are missing or invalid.

    Returns:
        tuple[int, int]: Computed (width, height) downscaling resolution.
    """
    if cfg is None or not isinstance(cfg, dict):
        return fallback

    try:
        leds_left = int(cfg.get("leds_left", 0))
        leds_top = int(cfg.get("leds_top", 0))
        leds_right = int(cfg.get("leds_right", 0))
        leds_bottom = int(cfg.get("leds_bottom", 0))
    except (ValueError, TypeError):
        return fallback

    # Validate ranges (guard against negative counts and extreme outlier values)
    if any(c < 0 or c > 10000 for c in (leds_left, leds_top, leds_right, leds_bottom)):
        return fallback

    horizontal_leds = max(leds_top, leds_bottom)
    vertical_leds = max(leds_left, leds_right)

    if horizontal_leds <= 0 or vertical_leds <= 0:
        return fallback

    if horizontal_leds > 10000 or vertical_leds > 10000:
        return fallback

    # Clamp oversampling factor to safe reasonable boundaries
    factor = max(1.0, min(8.0, float(oversample_factor)))

    target_w = int(round(horizontal_leds * factor))
    target_h = int(round(vertical_leds * factor))

    # Keep dimensions even for SIMD/video codec alignment
    target_w += target_w % 2
    target_h += target_h % 2

    # Clamp within configured minimum and maximum limits
    w = max(min_res[0], min(max_res[0], target_w))
    h = max(min_res[1], min(max_res[1], target_h))

    return (w, h)


def calculate_edge_depth_pixels(
    dimension: int,
    depth: float | int,
    max_ratio: float = 0.5,
    min_pixels: int = 1,
) -> int:
    """Calculate sampling depth in pixels along a specific dimension (width or height).

    Supports:
    - Relative ratio / percentage (0.0 to 0.5): Fraction of dimension.
    - Absolute pixel distance (>= 1): Directly specifies the depth in pixels.

    Edge Cases:
    - Overlap prevention: Clamped to at most max_ratio (default 50% of the dimension).
    - Minimum depth: Clamped to at least min_pixels (default 1 pixel).

    Args:
        dimension: Frame dimension (width or height) in pixels.
        depth: Relative float (e.g. 0.08) or integer pixel distance (e.g. 20).
        max_ratio: Maximum fraction allowed (default 0.5, preventing opposing overlap).
        min_pixels: Minimum pixel depth (default 1).

    Returns:
        int: Clamped sampling depth in pixels.
    """
    if dimension <= 0:
        return min_pixels

    if isinstance(depth, float) and 0.0 <= depth <= 1.0:
        clamped_ratio = max(0.0, min(max_ratio, depth))
        pixels = int(round(dimension * clamped_ratio))
    elif depth > 1.0 or isinstance(depth, int):
        pixels = int(round(depth))
    elif depth <= 0.0:
        pixels = min_pixels
    else:
        pixels = min_pixels

    max_allowed = max(min_pixels, int(dimension * max_ratio))
    return max(min_pixels, min(max_allowed, pixels))


def calculate_edge_rois(
    width: int,
    height: int,
    edge_depth: float | int | dict[str, float | int] = 0.08,
) -> dict[str, tuple[int, int, int, int]]:
    """Compute rectangular bounding boxes (x, y, w, h) for each screen edge margin.

    Args:
        width: Frame width in pixels.
        height: Frame height in pixels.
        edge_depth: Relative ratio (0.0 - 0.5), pixel distance (int/float >= 1),
            or a dict with specific depths for 'left', 'top', 'right', 'bottom'.

    Returns:
        dict[str, tuple[int, int, int, int]]: Dict mapping each edge to its (x, y, w, h) ROI.
    """
    if isinstance(edge_depth, dict):
        d_left = edge_depth.get("left", 0.08)
        d_top = edge_depth.get("top", 0.08)
        d_right = edge_depth.get("right", 0.08)
        d_bottom = edge_depth.get("bottom", 0.08)
    else:
        d_left = d_top = d_right = d_bottom = edge_depth

    px_left = calculate_edge_depth_pixels(width, d_left)
    px_top = calculate_edge_depth_pixels(height, d_top)
    px_right = calculate_edge_depth_pixels(width, d_right)
    px_bottom = calculate_edge_depth_pixels(height, d_bottom)

    return {
        "left": (0, 0, px_left, height),
        "top": (0, 0, width, px_top),
        "right": (max(0, width - px_right), 0, px_right, height),
        "bottom": (0, max(0, height - px_bottom), width, px_bottom),
    }


def calculate_segment_rois(
    width: int,
    height: int,
    edge: str,
    num_leds: int,
    edge_depth: float | int = 0.08,
) -> list[tuple[int, int, int, int]]:
    """Compute bounding boxes (x, y, w, h) for individual LED segments along an edge.

    Args:
        width: Frame width in pixels.
        height: Frame height in pixels.
        edge: Edge name ('left', 'top', 'right', or 'bottom').
        num_leds: Number of LEDs along this edge.
        edge_depth: Relative ratio or pixel depth.

    Returns:
        list[tuple[int, int, int, int]]: List of (x, y, w, h) ROIs for each LED segment.
    """
    if num_leds <= 0 or width <= 0 or height <= 0:
        return []

    depth_x = calculate_edge_depth_pixels(width, edge_depth)
    depth_y = calculate_edge_depth_pixels(height, edge_depth)

    rois: list[tuple[int, int, int, int]] = []
    if edge in ("top", "bottom"):
        y = 0 if edge == "top" else max(0, height - depth_y)
        h = depth_y
        for i in range(num_leds):
            x1 = int(round(i * width / num_leds))
            x2 = int(round((i + 1) * width / num_leds))
            seg_w = max(1, x2 - x1)
            rois.append((x1, y, seg_w, h))
    elif edge in ("left", "right"):
        x = 0 if edge == "left" else max(0, width - depth_x)
        w = depth_x
        for i in range(num_leds):
            y1 = int(round(i * height / num_leds))
            y2 = int(round((i + 1) * height / num_leds))
            seg_h = max(1, y2 - y1)
            rois.append((x, y1, w, seg_h))

    return rois


def apply_black_level_threshold(
    pixels: np.ndarray,
    threshold: float | int = 15,
    transition_range: float | int | None = None,
) -> np.ndarray:
    """Apply black level thresholding with smooth, non-linear fade-out to RGB pixels.

    Pixels whose maximum channel intensity is strictly below `threshold` are forced
    to (0, 0, 0), shutting down LEDs completely and preventing any residual white
    glow or standby light. Pixels in the transition zone [threshold, threshold + transition_range]
    undergo a continuous Hermite smoothstep fade-out to prevent flickering near the cutoff.
    Pixels above the transition knee remain unmodified.

    Args:
        pixels: uint8 NumPy array of shape (..., 3) or (N, 3) in RGB order.
        threshold: Black cutoff threshold (0..255 or float 0.0..1.0).
        transition_range: Width of smooth transition knee. Defaults to threshold or 12.0.

    Returns:
        np.ndarray: uint8 array of same shape with black cutoff applied.
    """
    if pixels.size == 0:
        return pixels

    # Normalize threshold to 0.0 .. 255.0 scale
    if isinstance(threshold, float) and 0.0 < threshold <= 1.0:
        thresh = threshold * 255.0
    else:
        thresh = float(threshold)
    thresh = max(0.0, min(255.0, thresh))

    if thresh <= 0.0:
        return pixels

    # Determine transition width
    if transition_range is None:
        trans_w = max(4.0, min(30.0, thresh))
    else:
        if isinstance(transition_range, float) and 0.0 < transition_range <= 1.0:
            trans_w = transition_range * 255.0
        else:
            trans_w = float(transition_range)
        trans_w = max(1.0, trans_w)

    knee = min(255.0, thresh + trans_w)

    # Use max channel intensity across RGB: ensures saturated pure colors
    # (e.g. pure blue with low BT.601 luma) are not erroneously blackened.
    pixels_float = pixels.astype(np.float32)
    intensity = np.max(pixels_float, axis=-1, keepdims=True)

    # Calculate smooth attenuation factor
    # factor = 0.0 when intensity <= thresh
    # factor = 1.0 when intensity >= knee
    # smoothstep curve between thresh and knee
    factor = np.zeros_like(intensity)
    in_transition = (intensity > thresh) & (intensity < knee)
    above_knee = intensity >= knee

    factor[above_knee] = 1.0
    if np.any(in_transition):
        t = (intensity[in_transition] - thresh) / (knee - thresh)
        # Hermite smoothstep: 3*t^2 - 2*t^3
        factor[in_transition] = t * t * (3.0 - 2.0 * t)

    attenuated = np.clip(np.round(pixels_float * factor), 0, 255).astype(np.uint8)
    return attenuated


class ScreenGrabber:
    """Abstracted screen capture backend providing unified grab() interface.

    Prioritizes DXCam (DirectX Desktop Duplication API) on Windows for zero-copy GPU capture,
    and seamlessly falls back to MSS for cross-platform compatibility.
    """

    def __init__(self, monitor_idx: int = 0) -> None:
        self.monitor_idx = monitor_idx
        self.backend_name = "none"
        self._cam: Any = None
        self._sct: Any = None
        self._init_grabber()

    def _init_grabber(self) -> None:
        if _HAS_DXCAM:
            try:
                # dxcam output_idx corresponds to monitor index
                self._cam = dxcam.create(output_idx=self.monitor_idx, output_color="RGB")
                if self._cam is not None:
                    self.backend_name = "dxcam"
                    return
            except Exception as e:
                print(f"[ScreenGrabber] DXCam init failed for display {self.monitor_idx}: {e}")

        if _HAS_MSS:
            try:
                mss_cls = getattr(mss, "MSS", mss.mss)
                self._sct = mss_cls()
                self.backend_name = "mss"
                return
            except Exception as e:
                print(f"[ScreenGrabber] MSS init failed for display {self.monitor_idx}: {e}")

    def grab(self) -> np.ndarray | None:
        """Grab the latest screen frame as an RGB numpy array (H, W, 3) uint8.

        Returns None if no new frame has been rendered by the display server.
        """
        if self.backend_name == "dxcam" and self._cam is not None:
            try:
                frame = self._cam.grab()
                return frame
            except Exception:
                return None

        elif self.backend_name == "mss" and self._sct is not None:
            try:
                monitors = self._sct.monitors
                # Index 0 in MSS is all monitors combined, 1..N are individual displays
                mss_idx = min(self.monitor_idx + 1, len(monitors) - 1)
                mon = monitors[mss_idx]
                sct_img = self._sct.grab(mon)
                # Convert BGRA to RGB array
                img_np = np.asarray(sct_img)
                # Fast RGB extraction (dropping alpha and swapping B and R)
                return cv2.cvtColor(img_np, cv2.COLOR_BGRA2RGB)
            except Exception:
                return None

        return None

    def release(self) -> None:
        if self._cam is not None:
            try:
                del self._cam
            except Exception:
                pass
            self._cam = None
        if self._sct is not None:
            try:
                self._sct.close()
            except Exception:
                pass
            self._sct = None


def process_monitor_frame(
    frame: np.ndarray,
    cfg: dict[str, Any],
    capture_res: tuple[int, int] | str | None = (128, 72),
    brightness: float = 1.0,
    saturation: float = 1.0,
    edge_depth: float | int | dict[str, float | int] = 0.08,
    black_threshold: float | int = 15,
    oversample_factor: float = 2.0,
) -> bytes:
    """Process a single monitor frame into raw RGB byte stream according to edge configuration.

    Uses SIMD-accelerated OpenCV operations with zero unnecessary heap allocations.
    Automatically applies adaptive resolution, edge sampling depth, and black level cutoff.

    Args:
        frame: Input image array (RGB, BGR, BGRA, or Grayscale).
        cfg: Monitor configuration dict.
        capture_res: Target (width, height) resolution or 'auto' / (0, 0) for adaptive scaling.
        brightness: Brightness multiplier.
        saturation: Color saturation multiplier.
        edge_depth: Sampling margin depth (relative ratio or pixel distance).
        black_threshold: Cutoff threshold below which RGB values are forced to (0, 0, 0).
        oversample_factor: Oversampling factor used when computing adaptive resolution.

    Returns:
        bytes: Raw contiguous RGB byte stream ready for DRGB protocol payload.
    """
    # 1. Normalize and sanitize input frame
    if frame is None or not isinstance(frame, np.ndarray) or frame.size == 0:
        target_res = (128, 72)
        if isinstance(capture_res, tuple) and capture_res[0] > 0 and capture_res[1] > 0:
            target_res = capture_res
        frame = np.zeros((target_res[1], target_res[0], 3), dtype=np.uint8)
    elif frame.ndim == 2:
        frame = cv2.cvtColor(frame, cv2.COLOR_GRAY2RGB)
    elif frame.ndim == 3:
        if frame.shape[2] == 4:
            # Safely drop alpha channel to avoid alpha bleeding into RGB channels
            frame = cv2.cvtColor(frame, cv2.COLOR_BGRA2RGB)
        elif frame.shape[2] == 1:
            frame = cv2.cvtColor(frame, cv2.COLOR_GRAY2RGB)
        elif frame.shape[2] > 4:
            frame = frame[:, :, :3]

    # 2. Determine target downscaling resolution (Adaptive vs Static)
    if (
        capture_res is None
        or capture_res == "auto"
        or (isinstance(capture_res, tuple) and (capture_res[0] <= 0 or capture_res[1] <= 0))
    ):
        eff_res = calculate_adaptive_resolution(cfg, oversample_factor=oversample_factor)
    elif isinstance(capture_res, str):
        eff_res = RESOLUTION_PRESETS.get(capture_res, (128, 72))
        if eff_res == (0, 0):
            eff_res = calculate_adaptive_resolution(cfg, oversample_factor=oversample_factor)
    else:
        eff_res = capture_res

    # 3. Downscale frame to target resolution
    thumb = cv2.resize(frame, eff_res, interpolation=cv2.INTER_LINEAR)

    # 4. Vectorized Saturation Adjustment
    if saturation != 1.0:
        hsv = cv2.cvtColor(thumb, cv2.COLOR_RGB2HSV)
        hsv[:, :, 1] = cv2.convertScaleAbs(hsv[:, :, 1], alpha=saturation)
        thumb = cv2.cvtColor(hsv, cv2.COLOR_HSV2RGB)

    # 5. Vectorized Brightness Scaling
    if brightness != 1.0:
        thumb = cv2.convertScaleAbs(thumb, alpha=brightness)

    h, w, _ = thumb.shape
    if isinstance(edge_depth, dict):
        d_left = edge_depth.get("left", 0.08)
        d_top = edge_depth.get("top", 0.08)
        d_right = edge_depth.get("right", 0.08)
        d_bottom = edge_depth.get("bottom", 0.08)
    else:
        d_left = d_top = d_right = d_bottom = edge_depth

    depth_x_left = calculate_edge_depth_pixels(w, d_left)
    depth_x_right = calculate_edge_depth_pixels(w, d_right)
    depth_y_top = calculate_edge_depth_pixels(h, d_top)
    depth_y_bottom = calculate_edge_depth_pixels(h, d_bottom)

    # 6. Extract edge bounding regions
    slice_left = thumb[0:h, 0:depth_x_left]
    slice_top = thumb[0:depth_y_top, 0:w]
    slice_right = thumb[0:h, max(0, w - depth_x_right):w]
    slice_bottom = thumb[max(0, h - depth_y_bottom):h, 0:w]

    leds_left = max(1, cfg.get("leds_left", 9))
    leds_top = max(1, cfg.get("leds_top", 15))
    leds_right = max(1, cfg.get("leds_right", 9))
    leds_bottom = max(1, cfg.get("leds_bottom", 15))

    # 7. Direct 1D area interpolation along each edge (properly averages the margin depth)
    edge_left = cv2.resize(slice_left, (1, leds_left), interpolation=cv2.INTER_AREA).reshape(-1, 3)
    edge_top = cv2.resize(slice_top, (leds_top, 1), interpolation=cv2.INTER_AREA).reshape(-1, 3)
    edge_right = cv2.resize(slice_right, (1, leds_right), interpolation=cv2.INTER_AREA).reshape(-1, 3)
    edge_bottom = cv2.resize(slice_bottom, (leds_bottom, 1), interpolation=cv2.INTER_AREA).reshape(-1, 3)

    # 8. Apply black level threshold & fade-out to eliminate residual white/glow
    if black_threshold > 0:
        edge_left = apply_black_level_threshold(edge_left, threshold=black_threshold)
        edge_top = apply_black_level_threshold(edge_top, threshold=black_threshold)
        edge_right = apply_black_level_threshold(edge_right, threshold=black_threshold)
        edge_bottom = apply_black_level_threshold(edge_bottom, threshold=black_threshold)

    directions = cfg.get("edge_directions", {"left": -1, "top": 1, "right": 1, "bottom": -1})
    edges: dict[str, np.ndarray] = {
        "left": edge_left[::-1] if directions.get("left", -1) == -1 else edge_left,
        "top": edge_top[::-1] if directions.get("top", 1) == -1 else edge_top,
        "right": edge_right[::-1] if directions.get("right", 1) == -1 else edge_right,
        "bottom": edge_bottom[::-1] if directions.get("bottom", -1) == -1 else edge_bottom,
    }

    order = cfg.get("edge_order", ["left", "top", "right", "bottom"])
    edge_bytes = bytearray()
    for edge_name in order:
        arr = edges.get(edge_name)
        if arr is not None:
            edge_bytes.extend(np.ascontiguousarray(arr, dtype=np.uint8).tobytes())

    return bytes(edge_bytes)


class WLEDWorker(threading.Thread):
    """Worker thread that continuously captures screen frames, computes edge colors,

    and streams DRGB packets over UDP to a WLED target device.
    """

    def __init__(
        self,
        ip: str,
        port: int,
        fps: int,
        monitor_configs: list[dict[str, Any]],
        brightness: float = 1.0,
        saturation: float = 1.0,
        resolution: str = "Balance (128x72)",
        smoothing: float = 0.0,
        edge_depth: float | int = 0.08,
        eco_mode: bool = True,
        black_threshold: float | int = 15,
        oversample_factor: float = 2.0,
    ) -> None:
        super().__init__()
        self.ip = ip
        self.port = int(port)
        self.fps = max(1, min(120, int(fps)))
        self.monitor_configs = monitor_configs
        self.brightness = float(brightness)
        self.saturation = float(saturation)
        self.resolution = resolution
        self.smoothing = max(0.0, min(0.95, float(smoothing)))
        self.edge_depth = edge_depth
        self.eco_mode = bool(eco_mode)
        self.black_threshold = float(black_threshold)
        self.oversample_factor = float(oversample_factor)

        self.is_running = False
        self.daemon = True

        self.sock: socket.socket | None = None
        self._prev_pixel_data: np.ndarray | None = None
        self._last_keepalive_time: float = 0.0

    def run(self) -> None:
        self.is_running = True

        try:
            self.sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        except Exception as e:
            print(f"[WLEDWorker] Failed to create UDP socket: {e}")
            self.is_running = False
            return

        # Initialize screen grabbers per unique monitor index
        grabbers: dict[int, ScreenGrabber] = {}
        for cfg in self.monitor_configs:
            idx = max(0, cfg.get("mss_index", 1) - 1)
            if idx not in grabbers:
                grabbers[idx] = ScreenGrabber(monitor_idx=idx)

        # Resolution preset lookup (or adaptive fallback)
        capture_res = RESOLUTION_PRESETS.get(self.resolution, (128, 72))
        last_packet = bytearray([2, 2])
        last_frames: dict[int, np.ndarray] = {}
        target_frame_time = 1.0 / self.fps

        while self.is_running:
            loop_start = time.perf_counter()

            frames_to_process: dict[int, np.ndarray] = {}
            has_new_frame = False

            # 1. Grab frames from displays
            for cfg in self.monitor_configs:
                idx = max(0, cfg.get("mss_index", 1) - 1)
                grabber = grabbers.get(idx)
                if not grabber:
                    continue

                frame = grabber.grab()
                if frame is not None:
                    last_frames[idx] = frame
                    frames_to_process[idx] = frame
                    has_new_frame = True
                else:
                    prev_frame = last_frames.get(idx)
                    if prev_frame is not None:
                        frames_to_process[idx] = prev_frame

            now = time.perf_counter()

            # 2. Eco Mode / Static Screen Optimization
            if not has_new_frame and len(last_packet) > 2 and self.eco_mode:
                if (now - self._last_keepalive_time) >= 1.0:
                    try:
                        self.sock.sendto(last_packet, (self.ip, self.port))
                        self._last_keepalive_time = now
                    except OSError:
                        pass

                elapsed = time.perf_counter() - loop_start
                sleep_duration = max(0.001, target_frame_time - elapsed)
                time.sleep(sleep_duration)
                continue

            # 3. Process frames and build DRGB packet
            packet = bytearray([2, 2])  # 2 = DRGB protocol, 2 = 2s timeout
            current_raw_bytes = bytearray()

            try:
                for cfg in self.monitor_configs:
                    idx = max(0, cfg.get("mss_index", 1) - 1)
                    frame = frames_to_process.get(idx)
                    if frame is None:
                        continue

                    mon_bytes = process_monitor_frame(
                        frame=frame,
                        cfg=cfg,
                        capture_res=capture_res,
                        brightness=self.brightness,
                        saturation=self.saturation,
                        edge_depth=self.edge_depth,
                        black_threshold=self.black_threshold,
                        oversample_factor=self.oversample_factor,
                    )
                    current_raw_bytes.extend(mon_bytes)

                if current_raw_bytes:
                    current_arr = np.frombuffer(current_raw_bytes, dtype=np.uint8)

                    # Smooth Transition Filter (exponential moving average)
                    if (
                        self.smoothing > 0.0
                        and self._prev_pixel_data is not None
                        and len(self._prev_pixel_data) == len(current_arr)
                    ):
                        smoothed = (
                            self.smoothing * self._prev_pixel_data.astype(np.float32)
                            + (1.0 - self.smoothing) * current_arr.astype(np.float32)
                        )
                        output_arr = np.clip(smoothed, 0, 255).astype(np.uint8)
                        # Re-apply black threshold after smoothing to snap down residual values
                        if self.black_threshold > 0:
                            output_arr = apply_black_level_threshold(
                                output_arr.reshape(-1, 3), threshold=self.black_threshold
                            ).reshape(-1)
                        self._prev_pixel_data = output_arr
                    else:
                        output_arr = current_arr
                        self._prev_pixel_data = current_arr

                    packet.extend(output_arr.tobytes())
                    last_packet = packet
                else:
                    packet = last_packet

            except Exception as e:
                print(f"[WLEDWorker] Frame processing exception: {e}")
                packet = last_packet

            # 4. Stream UDP Packet
            if self.is_running and len(packet) > 2:
                try:
                    self.sock.sendto(packet, (self.ip, self.port))
                    self._last_keepalive_time = now
                except OSError:
                    pass
                except Exception as e:
                    print(f"[WLEDWorker] UDP transmit error: {e}")

            # 5. Precise loop sleep
            elapsed = time.perf_counter() - loop_start
            sleep_duration = max(0.001, target_frame_time - elapsed)
            time.sleep(sleep_duration)

        # Cleanup resources
        for grabber in grabbers.values():
            try:
                grabber.release()
            except Exception:
                pass

        if self.sock is not None:
            try:
                self.sock.close()
            except Exception:
                pass
            self.sock = None

    def stop(self) -> None:
        """Signal the worker thread to terminate cleanly."""
        self.is_running = False
