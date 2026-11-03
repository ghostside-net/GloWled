"""
config.py - Configuration management, persistence, display discovery, and autostart for GloWled.
"""

from __future__ import annotations

import json
import os
import sys
from typing import Any

DEFAULT_CONFIG: dict[str, Any] = {
    "ip": "127.0.0.1",
    "port": 21324,
    "fps": 30,
    "brightness": 1.0,
    "saturation": 1.0,
    "smoothing": 0.1,
    "edge_depth": 0.08,
    "black_threshold": 15,
    "oversample_factor": 2.0,
    "eco_mode": True,
    "resolution": "Auto (Adaptiv)",
    "start_minimized": False,
    "monitors": [
        {
            "mss_index": 1,
            "leds_bottom": 15,
            "leds_left": 9,
            "leds_top": 15,
            "leds_right": 9,
            "edge_order": ["left", "top", "right", "bottom"],
            "edge_directions": {"left": -1, "top": 1, "right": 1, "bottom": -1},
        }
    ],
}


def resource_path(relative_path: str) -> str:
    """Return the absolute path to a resource, works for dev and PyInstaller bundle.

    Args:
        relative_path: Relative path string to the asset.

    Returns:
        str: Absolute file path to the resource.
    """
    try:
        base_path = sys._MEIPASS  # type: ignore[attr-defined]
    except AttributeError:
        base_path = os.path.abspath(os.path.dirname(__file__))
    return os.path.join(base_path, relative_path)


def get_app_data_dir() -> str:
    """Get the application data directory with support for legacy AmbilightController paths.

    Returns:
        str: Directory path where config.json is stored.
    """
    appdata = os.getenv("APPDATA")
    if appdata:
        glowled_dir = os.path.join(appdata, "GloWled")
        legacy_dir = os.path.join(appdata, "AmbilightController")
        # If legacy config exists and new directory doesn't yet have config, migrate seamlessly
        if not os.path.exists(os.path.join(glowled_dir, "config.json")) and os.path.exists(
            os.path.join(legacy_dir, "config.json")
        ):
            return legacy_dir
        return glowled_dir
    return os.path.join(os.path.expanduser("~"), ".glowled")


def get_config_file_path() -> str:
    """Return the absolute path to the active configuration file.

    Returns:
        str: Path to config.json.
    """
    return os.path.join(get_app_data_dir(), "config.json")


def load_config() -> dict[str, Any]:
    """Load configuration from JSON file or return defaults with schema fallback.

    Returns:
        dict[str, Any]: Configuration dictionary with all required keys.
    """
    config_path = get_config_file_path()
    cfg = DEFAULT_CONFIG.copy()

    if os.path.exists(config_path):
        try:
            with open(config_path, encoding="utf-8") as f:
                loaded = json.load(f)
                if isinstance(loaded, dict):
                    # Merge with default config to ensure new keys exist
                    for k, v in DEFAULT_CONFIG.items():
                        if k not in loaded:
                            loaded[k] = v
                    return loaded
        except (OSError, json.JSONDecodeError) as e:
            print(f"[Config] Error reading {config_path}: {e}. Using defaults.")

    return cfg


def save_config(config_dict: dict[str, Any]) -> bool:
    """Persist configuration dictionary to disk.

    Args:
        config_dict: Dictionary containing configuration key-value pairs.

    Returns:
        bool: True if saving succeeded, False otherwise.
    """
    config_dir = get_app_data_dir()
    config_path = os.path.join(config_dir, "config.json")
    try:
        if not os.path.exists(config_dir):
            os.makedirs(config_dir, exist_ok=True)
        with open(config_path, "w", encoding="utf-8") as f:
            json.dump(config_dict, f, indent=4)
        return True
    except OSError as e:
        print(f"[Config] Error saving config to {config_path}: {e}")
        return False


def get_autostart_path() -> str:
    """Return path to Windows Startup folder VBScript.

    Returns:
        str: Absolute path to startup VBS script.
    """
    appdata = os.getenv("APPDATA")
    if appdata:
        return os.path.join(
            appdata, "Microsoft", "Windows", "Start Menu", "Programs", "Startup", "GloWled.vbs"
        )
    return os.path.join(os.path.expanduser("~"), ".glowled_autostart.vbs")


def check_autostart() -> bool:
    """Check if autostart shortcut exists in Windows Startup folder.

    Returns:
        bool: True if startup script exists, False otherwise.
    """
    appdata = os.getenv("APPDATA")
    if appdata:
        legacy_path = os.path.join(
            appdata, "Microsoft", "Windows", "Start Menu", "Programs", "Startup", "AmbilightController.vbs"
        )
        if os.path.exists(legacy_path):
            return True
    return os.path.exists(get_autostart_path())


def toggle_autostart(enable: bool) -> bool:
    """Enable or disable Windows background startup entry.

    Args:
        enable: True to create startup shortcut, False to remove it.

    Returns:
        bool: True if operation completed successfully.
    """
    path = get_autostart_path()
    appdata = os.getenv("APPDATA")
    legacy_path = (
        os.path.join(appdata, "Microsoft", "Windows", "Start Menu", "Programs", "Startup", "AmbilightController.vbs")
        if appdata
        else None
    )

    if enable:
        if getattr(sys, "frozen", False):
            exec_path = sys.executable
            vbs_content = f'CreateObject("Wscript.Shell").Run """{exec_path}""", 0, False'
        else:
            script_path = os.path.abspath(sys.argv[0])
            python_path = sys.executable
            vbs_content = f'CreateObject("Wscript.Shell").Run """{python_path}"" ""{script_path}""", 0, False'

        try:
            parent_dir = os.path.dirname(path)
            if parent_dir and not os.path.exists(parent_dir):
                os.makedirs(parent_dir, exist_ok=True)
            with open(path, "w", encoding="utf-8") as f:
                f.write(vbs_content)
            # Remove legacy file if present to avoid duplicate starts
            if legacy_path and os.path.exists(legacy_path):
                try:
                    os.remove(legacy_path)
                except OSError:
                    pass
            return True
        except OSError as e:
            print(f"[Autostart] Error enabling autostart: {e}")
            return False
    else:
        removed = True
        for p in [path, legacy_path]:
            if p and os.path.exists(p):
                try:
                    os.remove(p)
                except OSError as e:
                    print(f"[Autostart] Error removing autostart entry {p}: {e}")
                    removed = False
        return removed


def detect_displays() -> list[dict[str, Any]]:
    """Detect available system monitors.

    Returns:
        list[dict[str, Any]]: List of dicts with display index, resolution string, and label.
    """
    displays: list[dict[str, Any]] = []
    try:
        import mss

        mss_cls = getattr(mss, "MSS", mss.mss)
        with mss_cls() as sct:
            # monitors[0] is virtual bounding box, monitors[1..] are individual displays
            for i, mon in enumerate(sct.monitors[1:], start=1):
                displays.append({
                    "mss_index": i,
                    "name": f"Monitor {i}",
                    "resolution": f"{mon['width']}x{mon['height']}",
                    "left": mon.get("left", 0),
                    "top": mon.get("top", 0),
                })
    except Exception:
        pass

    if not displays:
        # Fallback default display representation
        displays.append({
            "mss_index": 1,
            "name": "Monitor 1 (Default)",
            "resolution": "1920x1080",
            "left": 0,
            "top": 0,
        })

    return displays
