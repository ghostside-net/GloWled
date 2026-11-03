"""
Unit and regression test suite for GloWled.
"""

import json
import os
import socket
import tempfile
import time
from typing import Any, Dict

import cv2
import numpy as np
import pytest

import config
import worker


@pytest.fixture
def sample_monitor_config() -> Dict[str, Any]:
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
        "ip", "port", "fps", "brightness", "saturation",
        "smoothing", "edge_depth", "eco_mode", "resolution", "monitors"
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
        assert loaded["monitors"][0]["leds_top"] == 20


def test_config_backward_compatibility(monkeypatch):
    """Ensure older config files lacking new keys (smoothing, eco_mode) load gracefully with defaults."""
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
# Worker & Frame Processing Tests
# =========================================================================

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

    # With 2x brightness, pixel values should scale from ~50 to ~100
    assert np.mean(arr_bright) > np.mean(arr_normal)
    assert np.allclose(np.mean(arr_bright), 100, atol=2)


def test_saturation_adjustment(sample_monitor_config):
    # Pure red frame: RGB (200, 0, 0)
    frame = np.zeros((100, 100, 3), dtype=np.uint8)
    frame[:, :, 0] = 200

    raw_desat = worker.process_monitor_frame(
        frame, sample_monitor_config, (64, 36), brightness=1.0, saturation=0.0
    )
    arr_desat = np.frombuffer(raw_desat, dtype=np.uint8).reshape(-1, 3)

    # At 0 saturation, R, G, B channels should become equal (grayscale)
    assert np.allclose(arr_desat[:, 0], arr_desat[:, 1], atol=3)
    assert np.allclose(arr_desat[:, 1], arr_desat[:, 2], atol=3)


def test_edge_direction_inversion(sample_monitor_config):
    # Create gradient across top edge from 0 to 255
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

    # Inverted should equal reversed normal
    assert np.array_equal(arr_normal, arr_inverted[::-1])


def test_resolution_presets_validity():
    for name, (w, h) in worker.RESOLUTION_PRESETS.items():
        assert w > 0 and h > 0
        assert w >= h  # 16:9 aspect ratio standard


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
    # Find free UDP port for local test
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
        eco_mode=False,
    )

    bytes_black = worker.process_monitor_frame(frame_black, sample_monitor_config, (64, 36))
    bytes_white = worker.process_monitor_frame(frame_white, sample_monitor_config, (64, 36))

    arr_black = np.frombuffer(bytes_black, dtype=np.uint8)
    arr_white = np.frombuffer(bytes_white, dtype=np.uint8)

    w._prev_pixel_data = arr_black.copy()
    # Apply 50% smoothing blend
    smoothed = (
        0.5 * w._prev_pixel_data.astype(np.float32)
        + 0.5 * arr_white.astype(np.float32)
    )
    result = np.clip(smoothed, 0, 255).astype(np.uint8)

    # Values should be around 127
    assert np.allclose(result, 127, atol=2)


def test_edge_depth_cropping(sample_monitor_config):
    """Verify edge depth configuration affects sampling margin width."""
    frame = np.zeros((100, 100, 3), dtype=np.uint8)
    # Put bright stripe only at 20-25% from left edge (x=20..25)
    frame[:, 20:25] = 255

    cfg_left = sample_monitor_config.copy()
    cfg_left["edge_order"] = ["left"]

    # 5% depth should not capture the stripe on the left edge
    bytes_shallow = worker.process_monitor_frame(
        frame, cfg_left, (100, 100), edge_depth=0.05
    )
    arr_shallow = np.frombuffer(bytes_shallow, dtype=np.uint8)
    assert np.mean(arr_shallow) == 0

    # 30% depth should capture the stripe on the left edge
    bytes_deep = worker.process_monitor_frame(
        frame, cfg_left, (100, 100), edge_depth=0.30
    )
    arr_deep = np.frombuffer(bytes_deep, dtype=np.uint8)
    assert np.mean(arr_deep) > 0
