"""
config.py - Configuration management, persistence, display discovery, and autostart for GloWled.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
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
    "autostart": True,
    "last_known_exe_path": "",
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


def get_current_executable_path() -> str:
    """Return the absolute path to the current running executable or script.

    Returns:
        str: Absolute path to the executable (when frozen) or main python entry script.
    """
    if getattr(sys, "frozen", False):
        return os.path.abspath(sys.executable)
    if sys.argv and sys.argv[0]:
        return os.path.abspath(sys.argv[0])
    return os.path.abspath(sys.executable)


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


def get_start_menu_shortcut_path() -> str:
    """Return path to Windows Start Menu Programs shortcut.

    Returns:
        str: Absolute path to Start Menu .lnk shortcut.
    """
    appdata = os.getenv("APPDATA")
    if appdata:
        return os.path.join(
            appdata, "Microsoft", "Windows", "Start Menu", "Programs", "GloWled.lnk"
        )
    return os.path.join(os.path.expanduser("~"), ".glowled_startmenu.lnk")


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


def get_autostart_target() -> str | None:
    """Extract the executable path referenced inside the autostart VBScript.

    Returns:
        str | None: Absolute target path if found, None if file does not exist or cannot be parsed.
    """
    path = get_autostart_path()
    if not os.path.exists(path):
        return None
    try:
        with open(path, encoding="utf-8", errors="ignore") as f:
            content = f.read()
        # Look for explicit Run command patterns:
        # e.g., Run """<path>"" --autostart" or Run """<path>"""
        m = re.search(r'Run\s+"""([^"\n]+)"""', content)
        if m:
            return m.group(1).strip()
        m2 = re.search(r'Run\s+"""([^"\n]+)""\s+--autostart', content)
        if m2:
            return m2.group(1).strip()
        # Multi-quoted format: Run """<python>"" ""/<script>"" --autostart"
        m3 = re.findall(r'""([^"\n]+)""', content)
        if len(m3) >= 2:
            return m3[1].strip()
        elif m3:
            return m3[0].strip()
    except Exception as e:
        print(f"[Autostart] Error inspecting autostart target: {e}")
    return None


def create_or_update_start_menu_shortcut(target_path: str | None = None) -> bool:
    """Create or update the Windows Start Menu shortcut to point to the current executable.

    Args:
        target_path: Target executable path. Defaults to get_current_executable_path().

    Returns:
        bool: True if shortcut created or updated successfully, False on error or non-Windows.
    """
    if sys.platform != "win32":
        return False
    appdata = os.getenv("APPDATA")
    if not appdata:
        return False

    if target_path is None:
        target_path = get_current_executable_path()

    shortcut_path = get_start_menu_shortcut_path()
    icon_path = resource_path("icon.ico")
    work_dir = os.path.dirname(target_path)

    try:
        parent_dir = os.path.dirname(shortcut_path)
        if parent_dir and not os.path.exists(parent_dir):
            os.makedirs(parent_dir, exist_ok=True)

        ps_cmd = (
            f"$ws = New-Object -ComObject WScript.Shell; "
            f"$s = $ws.CreateShortcut('{shortcut_path}'); "
            f"$s.TargetPath = '{target_path}'; "
            f"$s.WorkingDirectory = '{work_dir}'; "
            f"$s.Description = 'GloWled — High-Performance WLED Ambilight Controller'; "
            + (f"$s.IconLocation = '{icon_path},0'; " if os.path.exists(icon_path) else "")
            + "$s.Save()"
        )
        subprocess.run(
            ["powershell", "-NoProfile", "-NonInteractive", "-Command", ps_cmd],
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
            timeout=5,
            check=False,
        )
        return True
    except Exception as e:
        print(f"[Shortcut] Failed to create Start Menu shortcut: {e}")
        return False


def toggle_autostart(enable: bool, exec_path: str | None = None) -> bool:
    """Enable or disable Windows background startup entry.

    Args:
        enable: True to create startup shortcut, False to remove it.
        exec_path: Optional explicit executable path. Defaults to get_current_executable_path().

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
        if exec_path is None:
            exec_path = get_current_executable_path()

        if getattr(sys, "frozen", False):
            vbs_content = f'CreateObject("Wscript.Shell").Run """{exec_path}"" --autostart", 0, False\n'
        else:
            python_path = sys.executable
            vbs_content = (
                f'CreateObject("Wscript.Shell").Run """{python_path}"" ""{exec_path}"" --autostart", 0, False\n'
            )

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


def sync_portable_environment(cfg: dict[str, Any]) -> dict[str, Any]:
    """Ensure complete setup on first launch and update paths if the executable was moved.

    Handles:
    - First start initialization (autostart, start menu shortcut, config persistence).
    - Executable folder migration: if GloWled is moved to another folder or drive,
      updates the Startup VBScript and Start Menu shortcuts automatically so that
      everything continues working seamlessly.
    - Legacy path cleanup (migrates old AmbilightController entries).

    Args:
        cfg: The currently loaded application configuration dictionary.

    Returns:
        dict[str, Any]: Status dict with keys:
            - is_first_run: bool
            - path_migrated: bool
            - previous_path: str
            - current_path: str
            - autostart_synced: bool
    """
    current_exec = get_current_executable_path()
    last_known = str(cfg.get("last_known_exe_path", "")).strip()

    is_first_run = not bool(last_known)
    path_migrated = False

    if not is_first_run:
        # Check case-insensitive and normalized paths for Windows compatibility
        if os.path.normcase(os.path.normpath(last_known)) != os.path.normcase(os.path.normpath(current_exec)):
            path_migrated = True

    # Also inspect existing autostart target in VBS
    existing_autostart_target = get_autostart_target()
    if existing_autostart_target and os.path.normcase(os.path.normpath(existing_autostart_target)) != os.path.normcase(
        os.path.normpath(current_exec)
    ):
        path_migrated = True

    autostart_desired = bool(cfg.get("autostart", True))
    autostart_synced = False

    # 1. On first run or if autostart is active/desired, ensure autostart points to current executable
    if is_first_run or path_migrated:
        if autostart_desired or check_autostart():
            toggle_autostart(True, current_exec)
            autostart_synced = True
            cfg["autostart"] = True

        # Update start menu shortcut on Windows
        if sys.platform == "win32":
            create_or_update_start_menu_shortcut(current_exec)

    elif autostart_desired and not check_autostart():
        # Autostart is desired according to config, but file is missing
        toggle_autostart(True, current_exec)
        autostart_synced = True

    # Update and persist last known executable path if it changed
    if cfg.get("last_known_exe_path") != current_exec or is_first_run:
        cfg["last_known_exe_path"] = current_exec
        save_config(cfg)

    return {
        "is_first_run": is_first_run,
        "path_migrated": path_migrated,
        "previous_path": last_known,
        "current_path": current_exec,
        "autostart_synced": autostart_synced,
    }


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
