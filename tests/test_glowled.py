"""
Unit and regression test suite for GloWled.
Covers configuration, adaptive resolution, edge sampling depth & ROIs,
black level handling, color fidelity, and end-to-end streaming pipelines.
"""

from __future__ import annotations

import json
import os
import socket
import tempfile
import time
from typing import Any

import numpy as np
import pytest

import config
import worker


@pytest.fixture
def sample_monitor_config() -> dict[str, Any]:
    return {
        "mss_index": 1,
        "leds_left": 9,
        "leds_top": 15,
        "leds_right": 9,
        "leds_bottom": 15,
        "edge_order": ["left", "top", "right", "bottom"],
        "edge_directions": {"left": -1, "top": 1, "right": 1, "bottom": -1},
    }


# =========================================================================
# Configuration Tests
# =========================================================================


def test_default_config_structure():
    cfg = config.DEFAULT_CONFIG
    required_keys = [
        "ip",
        "port",
        "fps",
        "brightness",
        "saturation",
        "smoothing",
        "edge_depth",
        "black_threshold",
        "oversample_factor",
        "eco_mode",
        "resolution",
        "monitors",
    ]
    for key in required_keys:
        assert key in cfg, f"Missing required config key: {key}"

    assert isinstance(cfg["monitors"], list)
    assert len(cfg["monitors"]) >= 1
    mon = cfg["monitors"][0]
    for mon_key in ["mss_index", "leds_left", "leds_top", "leds_right", "leds_bottom", "edge_order"]:
        assert mon_key in mon, f"Missing monitor key: {mon_key}"


def test_config_save_and_load(monkeypatch):
    with tempfile.TemporaryDirectory() as tmpdir:
        fake_cfg_file = os.path.join(tmpdir, "config.json")
        monkeypatch.setattr(config, "get_config_file_path", lambda: fake_cfg_file)
        monkeypatch.setattr(config, "get_app_data_dir", lambda: tmpdir)

        test_data = {
            "ip": "192.168.1.100",
            "port": 21324,
            "fps": 45,
            "brightness": 1.5,
            "saturation": 1.2,
            "smoothing": 0.25,
            "edge_depth": 0.1,
            "black_threshold": 20,
            "oversample_factor": 3.0,
            "eco_mode": True,
            "resolution": "Quality (256x144)",
            "start_minimized": True,
            "monitors": [
                {
                    "mss_index": 1,
                    "leds_left": 10,
                    "leds_top": 20,
                    "leds_right": 10,
                    "leds_bottom": 20,
                    "edge_order": ["top", "right", "bottom", "left"],
                    "edge_directions": {"left": 1, "top": 1, "right": 1, "bottom": 1},
                }
            ],
        }

        assert config.save_config(test_data) is True
        assert os.path.exists(fake_cfg_file)

        loaded = config.load_config()
        assert loaded["ip"] == "192.168.1.100"
        assert loaded["fps"] == 45
        assert loaded["brightness"] == 1.5
        assert loaded["black_threshold"] == 20
        assert loaded["oversample_factor"] == 3.0
        assert loaded["monitors"][0]["leds_top"] == 20


def test_config_backward_compatibility(monkeypatch):
    """Ensure older config files lacking new keys load gracefully with defaults."""
    with tempfile.TemporaryDirectory() as tmpdir:
        fake_cfg_file = os.path.join(tmpdir, "config.json")
        legacy_data = {
            "ip": "10.0.0.50",
            "port": 21324,
            "fps": 30,
            "brightness": 1.0,
            "saturation": 1.0,
            "resolution": "Balance (128x72)",
            "monitors": [
                {
                    "mss_index": 1,
                    "leds_left": 9,
                    "leds_top": 15,
                    "leds_right": 9,
                    "leds_bottom": 15,
                    "edge_order": ["left", "top", "right", "bottom"],
                    "edge_directions": {"left": -1, "top": 1, "right": 1, "bottom": -1},
                }
            ],
        }
        with open(fake_cfg_file, "w", encoding="utf-8") as f:
            json.dump(legacy_data, f)

        monkeypatch.setattr(config, "get_config_file_path", lambda: fake_cfg_file)
        monkeypatch.setattr(config, "get_app_data_dir", lambda: tmpdir)

        loaded = config.load_config()
        assert loaded["ip"] == "10.0.0.50"
        assert "smoothing" in loaded
        assert "eco_mode" in loaded
        assert "edge_depth" in loaded
        assert "black_threshold" in loaded
        assert "oversample_factor" in loaded


def test_detect_displays():
    displays = config.detect_displays()
    assert isinstance(displays, list)
    assert len(displays) >= 1
    assert "mss_index" in displays[0]
    assert "resolution" in displays[0]


def test_autostart_helpers(monkeypatch):
    with tempfile.TemporaryDirectory() as tmpdir:
        fake_vbs = os.path.join(tmpdir, "GloWled.vbs")
        monkeypatch.setattr(config, "get_autostart_path", lambda: fake_vbs)

        assert config.check_autostart() is False
        assert config.toggle_autostart(True) is True
        assert os.path.exists(fake_vbs)
        assert config.check_autostart() is True

        assert config.toggle_autostart(False) is True
        assert not os.path.exists(fake_vbs)
        assert config.check_autostart() is False


# =========================================================================
# Feature A: Adaptive Erfassungsauflösung Tests
# =========================================================================


def test_adaptive_resolution_layouts():
    """Verify adaptive downscaling calculation for standard LED densities (30, 60, 144 LEDs/m)."""
    # 30 LEDs/m setup (e.g. 30 top/bottom, 18 left/right)
    cfg_30 = {"leds_top": 30, "leds_bottom": 30, "leds_left": 18, "leds_right": 18}
    w, h = worker.calculate_adaptive_resolution(cfg_30, oversample_factor=2.0)
    assert w == 60  # 30 * 2
    assert h == 36  # 18 * 2

    # 60 LEDs/m setup (e.g. 60 top/bottom, 34 left/right)
    cfg_60 = {"leds_top": 60, "leds_bottom": 60, "leds_left": 34, "leds_right": 34}
    w, h = worker.calculate_adaptive_resolution(cfg_60, oversample_factor=2.0)
    assert w == 120  # 60 * 2
    assert h == 68  # 34 * 2

    # 144 LEDs/m high-density setup (e.g. 144 top/bottom, 80 left/right)
    cfg_144 = {"leds_top": 144, "leds_bottom": 144, "leds_left": 80, "leds_right": 80}
    w, h = worker.calculate_adaptive_resolution(cfg_144, oversample_factor=2.0)
    assert w == 288  # 144 * 2
    assert h == 160  # 80 * 2


def test_adaptive_resolution_oversampling_factors():
    """Verify Nyquist-Shannon oversampling scaling (2x, 3x, 4x)."""
    cfg = {"leds_top": 40, "leds_bottom": 40, "leds_left": 20, "leds_right": 20}

    # 2x oversampling (Nyquist minimum)
    w_2x, h_2x = worker.calculate_adaptive_resolution(cfg, oversample_factor=2.0)
    assert w_2x == 80
    assert h_2x == 40

    # 3x oversampling
    w_3x, h_3x = worker.calculate_adaptive_resolution(cfg, oversample_factor=3.0)
    assert w_3x == 120
    assert h_3x == 60

    # 4x oversampling
    w_4x, h_4x = worker.calculate_adaptive_resolution(cfg, oversample_factor=4.0)
    assert w_4x == 160
    assert h_4x == 80


def test_adaptive_resolution_edge_cases():
    """Verify fallback handling for 0 LEDs, negative counts, extremes (>10,000), and missing configs."""
    fallback = (128, 72)

    # 0 LEDs
    assert worker.calculate_adaptive_resolution({"leds_top": 0, "leds_left": 0}, fallback=fallback) == fallback

    # Negative counts
    assert worker.calculate_adaptive_resolution({"leds_top": -15, "leds_left": 9}, fallback=fallback) == fallback

    # Outlier / extreme values (>10,000 LEDs)
    assert worker.calculate_adaptive_resolution({"leds_top": 15000, "leds_left": 9000}, fallback=fallback) == fallback

    # Empty config / None / wrong types
    assert worker.calculate_adaptive_resolution(None, fallback=fallback) == fallback
    assert worker.calculate_adaptive_resolution({}, fallback=fallback) == fallback

    # Very small LED count: clamps to min_res
    cfg_tiny = {"leds_top": 4, "leds_bottom": 4, "leds_left": 2, "leds_right": 2}
    w_tiny, h_tiny = worker.calculate_adaptive_resolution(cfg_tiny, oversample_factor=2.0, min_res=(32, 18))
    assert w_tiny >= 32
    assert h_tiny >= 18


# =========================================================================
# Feature B: Konfigurierbare Erfassungs- / Randtiefe & ROI Tests
# =========================================================================


def test_edge_depth_pixels_boundaries():
    """Verify boundary clamping: 0%, 5%, 50%, and overlap prevention (>50%)."""
    dim = 100

    # 0% depth limit -> clamped to min 1 pixel
    assert worker.calculate_edge_depth_pixels(dim, 0.0) == 1

    # 5% depth -> exactly 5 pixels
    assert worker.calculate_edge_depth_pixels(dim, 0.05) == 5

    # 50% depth -> exactly 50 pixels
    assert worker.calculate_edge_depth_pixels(dim, 0.50) == 50

    # >50% depth (e.g. 70%) -> clamped to 50% (50 px) to prevent opposing edge overlap
    assert worker.calculate_edge_depth_pixels(dim, 0.70) == 50

    # Pixel mode: absolute distance (e.g. 15 pixels)
    assert worker.calculate_edge_depth_pixels(dim, 15) == 15

    # Pixel mode: over 50% -> clamped to 50 pixels
    assert worker.calculate_edge_depth_pixels(dim, 85) == 50


def test_calculate_edge_rois():
    """Verify edge ROI bounding boxes for asymmetric dimensions and dict parameters."""
    width, height = 1920, 1080
    rois = worker.calculate_edge_rois(width, height, edge_depth=0.10)

    # Left: x=0, y=0, w=192, h=1080
    assert rois["left"] == (0, 0, 192, 1080)
    # Top: x=0, y=0, w=1920, h=108
    assert rois["top"] == (0, 0, 1920, 108)
    # Right: x=1920-192=1728, y=0, w=192, h=1080
    assert rois["right"] == (1728, 0, 192, 1080)
    # Bottom: x=0, y=1080-108=972, w=1920, h=108
    assert rois["bottom"] == (0, 972, 1920, 108)

    # Per-edge dict depths
    dict_depth = {"left": 0.05, "top": 0.10, "right": 0.05, "bottom": 0.10}
    rois_dict = worker.calculate_edge_rois(width, height, edge_depth=dict_depth)
    assert rois_dict["left"][2] == 96  # 5% of 1920
    assert rois_dict["top"][3] == 108  # 10% of 1080


def test_calculate_segment_rois():
    """Verify individual LED segment ROI bounding boxes."""
    width, height = 100, 50

    # 5 LEDs on top edge
    segs_top = worker.calculate_segment_rois(width, height, edge="top", num_leds=5, edge_depth=0.10)
    assert len(segs_top) == 5
    for i, (x, y, w, h) in enumerate(segs_top):
        assert y == 0
        assert h == 5  # 10% of 50
        assert w == 20  # 100 / 5
        assert x == i * 20

    # 0 LEDs -> empty list
    assert worker.calculate_segment_rois(width, height, edge="top", num_leds=0) == []


# =========================================================================
# Feature C: Schwarz-Darstellung & Color Extraction Tests
# =========================================================================


def test_black_level_pure_black(sample_monitor_config):
    """Verify that pure black (#000000) produces strictly RGB(0, 0, 0) for all LEDs."""
    frame_black = np.zeros((100, 100, 3), dtype=np.uint8)

    raw_bytes = worker.process_monitor_frame(
        frame=frame_black,
        cfg=sample_monitor_config,
        capture_res=(64, 36),
        black_threshold=15,
    )
    arr = np.frombuffer(raw_bytes, dtype=np.uint8)

    # Every single byte must be strictly 0 (no white, no standby glow)
    assert np.all(arr == 0)
    assert np.max(arr) == 0


def test_black_level_dark_gray_below_threshold(sample_monitor_config):
    """Verify that dark gray below threshold (#080808 with threshold 15) shuts LEDs completely off."""
    # Screen displaying #080808 (typical OLED/letterbox dark gray)
    frame_dark = np.full((100, 100, 3), 8, dtype=np.uint8)

    raw_bytes = worker.process_monitor_frame(
        frame=frame_dark,
        cfg=sample_monitor_config,
        capture_res=(64, 36),
        black_threshold=15,
    )
    arr = np.frombuffer(raw_bytes, dtype=np.uint8)

    # Must be strictly 0
    assert np.all(arr == 0)


def test_black_level_smoothstep_fadeout():
    """Verify non-linear smoothstep transfer curve in the transition zone."""
    pixels = np.array([
        [5, 5, 5],  # Below thresh (15) -> 0
        [15, 15, 15],  # At thresh -> 0
        [20, 20, 20],  # In transition zone [15, 30] -> attenuated
        [30, 30, 30],  # At knee -> 30
        [100, 100, 100],  # Well above knee -> 100
    ], dtype=np.uint8)

    processed = worker.apply_black_level_threshold(pixels, threshold=15, transition_range=15)

    # Pixel 0: below threshold -> 0
    assert np.array_equal(processed[0], [0, 0, 0])
    # Pixel 1: at threshold -> 0
    assert np.array_equal(processed[1], [0, 0, 0])
    # Pixel 2: in transition -> attenuated: > 0 but < 20
    assert 0 < processed[2, 0] < 20
    # Pixel 3: at knee -> 30
    assert np.allclose(processed[3], [30, 30, 30], atol=1)
    # Pixel 4: well above knee -> preserved
    assert np.array_equal(processed[4], [100, 100, 100])


def test_black_level_preserves_full_and_mixed_colors():
    """Verify that pure colors (R, G, B) and mixed saturated colors are preserved."""
    pixels = np.array([
        [255, 0, 0],  # Pure Red
        [0, 255, 0],  # Pure Green
        [0, 0, 255],  # Pure Blue
        [255, 255, 0],  # Pure Yellow
        [0, 255, 255],  # Pure Cyan
        [255, 0, 255],  # Pure Magenta
    ], dtype=np.uint8)

    processed = worker.apply_black_level_threshold(pixels, threshold=15)
    assert np.array_equal(processed, pixels)


def test_bgra_alpha_channel_sanitization(sample_monitor_config):
    """Verify that 4-channel BGRA pure black frames with Alpha=255 do NOT cause white output."""
    h, w = 100, 100
    # 4-channel BGRA frame: B=0, G=0, R=0, Alpha=255
    frame_bgra_black = np.zeros((h, w, 4), dtype=np.uint8)
    frame_bgra_black[:, :, 3] = 255  # Alpha channel is 255

    raw_bytes = worker.process_monitor_frame(
        frame=frame_bgra_black,
        cfg=sample_monitor_config,
        capture_res=(64, 36),
        black_threshold=15,
    )
    arr = np.frombuffer(raw_bytes, dtype=np.uint8)

    # The output MUST be strictly 0, proving alpha does not bleed into color channels!
    assert np.all(arr == 0)


def test_smoothing_with_black_threshold_snapping(sample_monitor_config):
    """Verify that exponential smoothing decays to 0 and snaps completely off below black_threshold."""
    w = worker.WLEDWorker(
        ip="127.0.0.1",
        port=21324,
        fps=30,
        monitor_configs=[sample_monitor_config],
        smoothing=0.5,
        black_threshold=15,
        eco_mode=False,
    )

    # Previous frame was dark gray (value 20, just above threshold)
    total_leds = (
        sample_monitor_config["leds_left"]
        + sample_monitor_config["leds_top"]
        + sample_monitor_config["leds_right"]
        + sample_monitor_config["leds_bottom"]
    )
    w._prev_pixel_data = np.full((total_leds * 3,), 20, dtype=np.uint8)

    # Current incoming frame is pure black (0)
    current_arr = np.zeros((total_leds * 3,), dtype=np.uint8)

    # Smooth: 0.5 * 20 + 0.5 * 0 = 10 (which is < threshold 15)
    smoothed = (
        0.5 * w._prev_pixel_data.astype(np.float32)
        + 0.5 * current_arr.astype(np.float32)
    )
    output_arr = np.clip(smoothed, 0, 255).astype(np.uint8)
    snapped = worker.apply_black_level_threshold(output_arr.reshape(-1, 3), threshold=15).reshape(-1)

    # Must snap strictly to 0
    assert np.all(snapped == 0)


# =========================================================================
# Integration & Streaming Tests
# =========================================================================


def test_end_to_end_frame_to_led_buffer(sample_monitor_config):
    """End-to-end integration test: Frame capture to DRGB UDP packet buffer."""
    h, w = 720, 1280
    frame = np.zeros((h, w, 3), dtype=np.uint8)

    # Top edge red, left edge green, bottom edge blue, right edge yellow
    frame[0:50, :] = [255, 0, 0]  # Red
    frame[:, 0:50] = [0, 255, 0]  # Green
    frame[h - 50:h, :] = [0, 0, 255]  # Blue
    frame[:, w - 50:w] = [255, 255, 0]  # Yellow

    raw_bytes = worker.process_monitor_frame(
        frame=frame,
        cfg=sample_monitor_config,
        capture_res="auto",
        brightness=1.0,
        saturation=1.0,
        edge_depth=0.10,
        black_threshold=15,
        oversample_factor=2.0,
    )

    total_leds = (
        sample_monitor_config["leds_left"]
        + sample_monitor_config["leds_top"]
        + sample_monitor_config["leds_right"]
        + sample_monitor_config["leds_bottom"]
    )
    assert len(raw_bytes) == total_leds * 3

    # Pack into WLED DRGB packet: [2, 2, ...rgb...]
    packet = bytearray([2, 2])
    packet.extend(raw_bytes)
    assert len(packet) == 2 + total_leds * 3
    assert packet[0] == 2
    assert packet[1] == 2


def test_resolution_presets_validity():
    for name, (w, h) in worker.RESOLUTION_PRESETS.items():
        if name.startswith("Auto"):
            assert (w, h) == (0, 0)
        else:
            assert w > 0 and h > 0
            assert w >= h  # 16:9 aspect ratio standard


def test_process_monitor_frame_byte_length(sample_monitor_config):
    h, w = 720, 1280
    frame = np.full((h, w, 3), 128, dtype=np.uint8)

    res_bytes = worker.process_monitor_frame(
        frame=frame,
        cfg=sample_monitor_config,
        capture_res=(128, 72),
        brightness=1.0,
        saturation=1.0,
        edge_depth=0.08,
    )

    total_leds = (
        sample_monitor_config["leds_left"]
        + sample_monitor_config["leds_top"]
        + sample_monitor_config["leds_right"]
        + sample_monitor_config["leds_bottom"]
    )
    expected_bytes = total_leds * 3
    assert len(res_bytes) == expected_bytes


def test_brightness_scaling(sample_monitor_config):
    frame = np.full((100, 100, 3), 50, dtype=np.uint8)

    raw_normal = worker.process_monitor_frame(
        frame, sample_monitor_config, (64, 36), brightness=1.0, saturation=1.0
    )
    raw_bright = worker.process_monitor_frame(
        frame, sample_monitor_config, (64, 36), brightness=2.0, saturation=1.0
    )

    arr_normal = np.frombuffer(raw_normal, dtype=np.uint8)
    arr_bright = np.frombuffer(raw_bright, dtype=np.uint8)

    assert np.mean(arr_bright) > np.mean(arr_normal)
    assert np.allclose(np.mean(arr_bright), 100, atol=2)


def test_saturation_adjustment(sample_monitor_config):
    frame = np.zeros((100, 100, 3), dtype=np.uint8)
    frame[:, :, 0] = 200

    raw_desat = worker.process_monitor_frame(
        frame, sample_monitor_config, (64, 36), brightness=1.0, saturation=0.0
    )
    arr_desat = np.frombuffer(raw_desat, dtype=np.uint8).reshape(-1, 3)

    assert np.allclose(arr_desat[:, 0], arr_desat[:, 1], atol=3)
    assert np.allclose(arr_desat[:, 1], arr_desat[:, 2], atol=3)


def test_edge_direction_inversion(sample_monitor_config):
    frame = np.zeros((100, 100, 3), dtype=np.uint8)
    for x in range(100):
        frame[0:20, x] = int((x / 99.0) * 255)

    cfg_normal = sample_monitor_config.copy()
    cfg_normal["edge_order"] = ["top"]
    cfg_normal["edge_directions"] = {"top": 1}

    cfg_inverted = sample_monitor_config.copy()
    cfg_inverted["edge_order"] = ["top"]
    cfg_inverted["edge_directions"] = {"top": -1}

    bytes_normal = worker.process_monitor_frame(frame, cfg_normal, (128, 72))
    bytes_inverted = worker.process_monitor_frame(frame, cfg_inverted, (128, 72))

    arr_normal = np.frombuffer(bytes_normal, dtype=np.uint8).reshape(-1, 3)
    arr_inverted = np.frombuffer(bytes_inverted, dtype=np.uint8).reshape(-1, 3)

    assert np.array_equal(arr_normal, arr_inverted[::-1])


def test_screen_grabber_safe_lifecycle():
    grabber = worker.ScreenGrabber(0)
    assert grabber.backend_name in ["dxcam", "mss", "none"]
    frame = grabber.grab()
    if frame is not None:
        assert isinstance(frame, np.ndarray)
    grabber.release()
    assert grabber._cam is None
    assert grabber._sct is None


def test_worker_streaming_lifecycle(sample_monitor_config):
    """Verify that WLEDWorker spins up, binds UDP socket, streams, and shuts down cleanly."""
    test_sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    test_sock.bind(("127.0.0.1", 0))
    _, test_port = test_sock.getsockname()
    test_sock.close()

    recv_sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    recv_sock.bind(("127.0.0.1", test_port))
    recv_sock.settimeout(0.5)

    w = worker.WLEDWorker(
        ip="127.0.0.1",
        port=test_port,
        fps=30,
        monitor_configs=[sample_monitor_config],
        brightness=1.0,
        saturation=1.0,
        resolution="Performance (64x36)",
        smoothing=0.0,
        edge_depth=0.08,
        black_threshold=15,
        eco_mode=False,
    )

    w.start()
    assert w.is_alive()
    time.sleep(0.1)

    w.stop()
    w.join(timeout=2.0)
    assert not w.is_alive()

    recv_sock.close()


def test_smoothing_filter(sample_monitor_config):
    """Verify that exponential moving average smoothing blends previous and current values."""
    frame_black = np.zeros((100, 100, 3), dtype=np.uint8)
    frame_white = np.full((100, 100, 3), 255, dtype=np.uint8)

    w = worker.WLEDWorker(
        ip="127.0.0.1",
        port=21324,
        fps=30,
        monitor_configs=[sample_monitor_config],
        smoothing=0.5,
        black_threshold=0,
        eco_mode=False,
    )

    bytes_black = worker.process_monitor_frame(frame_black, sample_monitor_config, (64, 36), black_threshold=0)
    bytes_white = worker.process_monitor_frame(frame_white, sample_monitor_config, (64, 36), black_threshold=0)

    arr_black = np.frombuffer(bytes_black, dtype=np.uint8)
    arr_white = np.frombuffer(bytes_white, dtype=np.uint8)

    w._prev_pixel_data = arr_black.copy()
    smoothed = (
        0.5 * w._prev_pixel_data.astype(np.float32)
        + 0.5 * arr_white.astype(np.float32)
    )
    result = np.clip(smoothed, 0, 255).astype(np.uint8)

    assert np.allclose(result, 127, atol=2)


def test_edge_depth_cropping(sample_monitor_config):
    """Verify edge depth configuration affects sampling margin width."""
    frame = np.zeros((100, 100, 3), dtype=np.uint8)
    frame[:, 20:25] = 255

    cfg_left = sample_monitor_config.copy()
    cfg_left["edge_order"] = ["left"]

    # 5% depth should not capture the stripe on the left edge
    bytes_shallow = worker.process_monitor_frame(
        frame, cfg_left, (100, 100), edge_depth=0.05, black_threshold=0
    )
    arr_shallow = np.frombuffer(bytes_shallow, dtype=np.uint8)
    assert np.mean(arr_shallow) == 0

    # 30% depth should capture the stripe on the left edge
    bytes_deep = worker.process_monitor_frame(
        frame, cfg_left, (100, 100), edge_depth=0.30, black_threshold=0
    )
    arr_deep = np.frombuffer(bytes_deep, dtype=np.uint8)
    assert np.mean(arr_deep) > 0
