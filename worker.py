"""
worker.py - Ultra-lightweight, high-performance WLED Ambilight Capture Worker.
Optimized for minimal CPU and memory footprint with zero-copy SIMD processing.
"""

import socket
import sys
import threading
import time
from typing import Any, Dict, List, Optional, Tuple

import cv2
import numpy as np

# Resolution lookup table (cached at module level)
RESOLUTION_PRESETS: Dict[str, Tuple[int, int]] = {
    "Performance (64x36)": (64, 36),
    "Balance (128x72)": (128, 72),
    "Balanced (128x72)": (128, 72),
    "Quality (256x144)": (256, 144),
    "Ultra (512x288)": (512, 288),
}

# Optional high-performance DirectX capture for Windows
_HAS_DXCAM = False
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


class ScreenGrabber:
    """
    Abstracted screen capture backend providing unified grab() interface.
    Prioritizes DXCam (DirectX Desktop Duplication API) on Windows for zero-copy GPU capture,
    and seamlessly falls back to MSS for cross-platform compatibility.
    """

    def __init__(self, monitor_idx: int = 0):
        self.monitor_idx = monitor_idx
        self.backend_name = "none"
        self._cam = None
        self._sct = None
        self._init_grabber()

    def _init_grabber(self):
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

    def grab(self) -> Optional[np.ndarray]:
        """
        Grab the latest screen frame as an RGB numpy array (H, W, 3) uint8.
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

    def release(self):
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
    cfg: Dict[str, Any],
    capture_res: Tuple[int, int],
    brightness: float = 1.0,
    saturation: float = 1.0,
    edge_depth: float = 0.08,
) -> bytes:
    """
    Process a single monitor frame into raw RGB byte stream according to edge configuration.
    Uses SIMD-accelerated OpenCV operations with zero unnecessary heap allocations.
    """
    # 1. Downscale frame to target resolution
    thumb = cv2.resize(frame, capture_res, interpolation=cv2.INTER_LINEAR)

    # 2. Vectorized Saturation Adjustment (operates in uint8 without float allocations)
    if saturation != 1.0:
        hsv = cv2.cvtColor(thumb, cv2.COLOR_RGB2HSV)
        hsv[:, :, 1] = cv2.convertScaleAbs(hsv[:, :, 1], alpha=saturation)
        thumb = cv2.cvtColor(hsv, cv2.COLOR_HSV2RGB)

    # 3. Vectorized Brightness Scaling
    if brightness != 1.0:
        thumb = cv2.convertScaleAbs(thumb, alpha=brightness)

    h, w, _ = thumb.shape
    depth_x = max(1, int(w * edge_depth))
    depth_y = max(1, int(h * edge_depth))

    # 4. Extract edge bounding regions
    slice_left = thumb[0:h, 0:depth_x]
    slice_top = thumb[0:depth_y, 0:w]
    slice_right = thumb[0:h, max(0, w - depth_x):w]
    slice_bottom = thumb[max(0, h - depth_y):h, 0:w]

    leds_left = max(1, cfg.get("leds_left", 9))
    leds_top = max(1, cfg.get("leds_top", 15))
    leds_right = max(1, cfg.get("leds_right", 9))
    leds_bottom = max(1, cfg.get("leds_bottom", 15))

    # 5. Direct 1D area interpolation along each edge (properly averages the margin depth)
    edge_left = cv2.resize(slice_left, (1, leds_left), interpolation=cv2.INTER_AREA).reshape(-1, 3)
    edge_top = cv2.resize(slice_top, (leds_top, 1), interpolation=cv2.INTER_AREA).reshape(-1, 3)
    edge_right = cv2.resize(slice_right, (1, leds_right), interpolation=cv2.INTER_AREA).reshape(-1, 3)
    edge_bottom = cv2.resize(slice_bottom, (leds_bottom, 1), interpolation=cv2.INTER_AREA).reshape(-1, 3)

    directions = cfg.get("edge_directions", {"left": -1, "top": 1, "right": 1, "bottom": -1})
    edges: Dict[str, np.ndarray] = {
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
    """
    Worker thread that continuously captures screen frames, computes edge colors,
    and streams DRGB packets over UDP to a WLED target device.
    """

    def __init__(
        self,
        ip: str,
        port: int,
        fps: int,
        monitor_configs: List[Dict[str, Any]],
        brightness: float = 1.0,
        saturation: float = 1.0,
        resolution: str = "Balance (128x72)",
        smoothing: float = 0.0,
        edge_depth: float = 0.08,
        eco_mode: bool = True,
    ):
        super().__init__()
        self.ip = ip
        self.port = int(port)
        self.fps = max(1, min(120, int(fps)))
        self.monitor_configs = monitor_configs
        self.brightness = float(brightness)
        self.saturation = float(saturation)
        self.resolution = resolution
        self.smoothing = max(0.0, min(0.95, float(smoothing)))
        self.edge_depth = max(0.02, min(0.35, float(edge_depth)))
        self.eco_mode = bool(eco_mode)

        self.is_running = False
        self.daemon = True

        self.sock: Optional[socket.socket] = None
        self._prev_pixel_data: Optional[np.ndarray] = None
        self._last_keepalive_time: float = 0.0

    def run(self):
        self.is_running = True

        try:
            self.sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        except Exception as e:
            print(f"[WLEDWorker] Failed to create UDP socket: {e}")
            self.is_running = False
            return

        # Initialize screen grabbers per unique monitor index
        grabbers: Dict[int, ScreenGrabber] = {}
        for cfg in self.monitor_configs:
            idx = max(0, cfg.get("mss_index", 1) - 1)
            if idx not in grabbers:
                grabbers[idx] = ScreenGrabber(monitor_idx=idx)

        capture_res = RESOLUTION_PRESETS.get(self.resolution, (128, 72))
        last_packet = bytearray([2, 2])
        last_frames: Dict[int, np.ndarray] = {}
        target_frame_time = 1.0 / self.fps

        while self.is_running:
            loop_start = time.perf_counter()

            frames_to_process: Dict[int, np.ndarray] = {}
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
                    frames_to_process[idx] = last_frames.get(idx)

            now = time.perf_counter()

            # 2. Eco Mode / Static Screen Optimization:
            # If the screen hasn't changed, send a keep-alive only once per second.
            # This cuts CPU and network bandwidth down to ~0% while reading or pausing video.
            if not has_new_frame and len(last_packet) > 2 and self.eco_mode:
                if (now - self._last_keepalive_time) >= 1.0:
                    try:
                        self.sock.sendto(last_packet, (self.ip, self.port))
                        self._last_keepalive_time = now
                    except OSError:
                        pass
                
                # Precise sleep until next frame slot
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
                    )
                    current_raw_bytes.extend(mon_bytes)

                if current_raw_bytes:
                    current_arr = np.frombuffer(current_raw_bytes, dtype=np.uint8)

                    # Optional Smooth Transition Filter (exponential moving average)
                    if self.smoothing > 0.0 and self._prev_pixel_data is not None and len(self._prev_pixel_data) == len(current_arr):
                        smoothed = (
                            self.smoothing * self._prev_pixel_data.astype(np.float32)
                            + (1.0 - self.smoothing) * current_arr.astype(np.float32)
                        )
                        output_arr = np.clip(smoothed, 0, 255).astype(np.uint8)
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

    def stop(self):
        """Signal the worker thread to terminate cleanly."""
        self.is_running = False
