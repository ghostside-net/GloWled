"""
main.py - GloWled: Modern, ultra-lightweight WLED Ambient Lighting Controller.
Features a sleek, intuitive CustomTkinter UI, system tray integration, and low-latency streaming.
"""

import argparse
import os
import socket
import sys
import threading
import time
from typing import Any

from config import (
    DEFAULT_CONFIG,
    check_autostart,
    detect_displays,
    get_config_file_path,
    get_current_executable_path,
    load_config,
    resource_path,
    save_config,
    sync_portable_environment,
    toggle_autostart,
)
from worker import RESOLUTION_PRESETS, WLEDWorker


def acquire_single_instance_lock() -> Any:
    """Acquire single-instance mutex on Windows to prevent duplicate streaming workers."""
    if sys.platform != "win32":
        return True
    try:
        import ctypes

        mutex_name = "Global\\GloWled_SingleInstance_Mutex_v1"
        mutex = ctypes.windll.kernel32.CreateMutexW(None, False, mutex_name)
        last_error = ctypes.windll.kernel32.GetLastError()
        error_already_exists = 183
        if last_error == error_already_exists:
            return None
        return mutex
    except Exception:
        return True


def bring_existing_instance_to_front() -> None:
    """Restore and bring existing GloWled application window to foreground on Windows."""
    if sys.platform != "win32":
        return
    try:
        import ctypes
        from ctypes import wintypes

        user32 = ctypes.windll.user32
        target_hwnd = None

        def enum_windows_proc(hwnd: Any, lparam: Any) -> bool:
            nonlocal target_hwnd
            length = user32.GetWindowTextLengthW(hwnd)
            if length > 0:
                buff = ctypes.create_unicode_buffer(length + 1)
                user32.GetWindowTextW(hwnd, buff, length + 1)
                if "GloWled" in buff.value:
                    target_hwnd = hwnd
                    return False
            return True

        wnd_enum_proc_type = ctypes.WINFUNCTYPE(ctypes.c_bool, wintypes.HWND, wintypes.LPARAM)
        user32.EnumWindows(wnd_enum_proc_type(enum_windows_proc), 0)

        if target_hwnd:
            user32.ShowWindow(target_hwnd, 9)  # SW_RESTORE
            user32.SetForegroundWindow(target_hwnd)
    except Exception:
        pass

# Guard CustomTkinter and GUI imports for headless compatibility
_GUI_AVAILABLE = False
try:
    import customtkinter as ctk
    import pystray
    from PIL import Image, ImageDraw
    _GUI_AVAILABLE = True
except Exception:
    _GUI_AVAILABLE = False


if _GUI_AVAILABLE:
    ctk.set_appearance_mode("dark")
    ctk.set_default_color_theme("blue")


class MonitorCard(ctk.CTkFrame if _GUI_AVAILABLE else object):  # type: ignore[misc]
    """
    Polished card component for individual monitor configuration,
    including LED counts, edge order presets, and direction inversion.
    """

    def __init__(self, master, config_data: dict[str, Any], remove_callback, **kwargs):
        if not _GUI_AVAILABLE:
            return
        super().__init__(
            master,
            fg_color="#18181b",
            corner_radius=10,
            border_width=1,
            border_color="#27272a",
            **kwargs,
        )
        self.remove_callback = remove_callback
        self.config_data = config_data.copy()

        # Header with Title and Remove Button
        self.header = ctk.CTkFrame(self, fg_color="transparent")
        self.header.pack(fill="x", padx=14, pady=(12, 6))

        mss_idx = config_data.get("mss_index", 1)
        self.title_label = ctk.CTkLabel(
            self.header,
            text=f"🖥️ Monitor {mss_idx}",
            font=ctk.CTkFont(size=14, weight="bold"),
            text_color="#f4f4f5",
        )
        self.title_label.pack(side="left")

        self.del_btn = ctk.CTkButton(
            self.header,
            text="Entfernen",
            width=80,
            height=28,
            fg_color="#dc2626",
            hover_color="#991b1b",
            font=ctk.CTkFont(size=12),
            command=self.on_remove,
        )
        self.del_btn.pack(side="right")

        # Monitor Index Selector
        self.idx_row = ctk.CTkFrame(self, fg_color="transparent")
        self.idx_row.pack(fill="x", padx=14, pady=4)
        ctk.CTkLabel(
            self.idx_row,
            text="Display Index (MSS/DXCam):",
            font=ctk.CTkFont(size=12),
            text_color="#a1a1aa",
        ).pack(side="left")

        self.idx_entry = ctk.CTkEntry(self.idx_row, width=60, height=28, justify="center")
        self.idx_entry.insert(0, str(mss_idx))
        self.idx_entry.pack(side="right")

        # LED Counts (Left, Top, Right, Bottom)
        self.led_box = ctk.CTkFrame(self, fg_color="#27272a", corner_radius=8)
        self.led_box.pack(fill="x", padx=14, pady=8)

        grid_frame = ctk.CTkFrame(self.led_box, fg_color="transparent")
        grid_frame.pack(fill="x", padx=10, pady=8)

        ctk.CTkLabel(
            grid_frame, text="LEDs pro Kante:", font=ctk.CTkFont(size=12, weight="bold"), text_color="#e4e4e7"
        ).pack(side="top", anchor="w", pady=(0, 6))

        inputs_row = ctk.CTkFrame(grid_frame, fg_color="transparent")
        inputs_row.pack(fill="x")

        # Links (Left)
        ctk.CTkLabel(inputs_row, text="Links:", font=ctk.CTkFont(size=11), text_color="#a1a1aa").pack(
            side="left", padx=(0, 2)
        )
        self.led_left = ctk.CTkEntry(inputs_row, width=44, height=28, justify="center")
        self.led_left.insert(0, str(config_data.get("leds_left", 9)))
        self.led_left.pack(side="left", padx=(0, 10))

        # Oben (Top)
        ctk.CTkLabel(inputs_row, text="Oben:", font=ctk.CTkFont(size=11), text_color="#a1a1aa").pack(
            side="left", padx=(0, 2)
        )
        self.led_top = ctk.CTkEntry(inputs_row, width=44, height=28, justify="center")
        self.led_top.insert(0, str(config_data.get("leds_top", 15)))
        self.led_top.pack(side="left", padx=(0, 10))

        # Rechts (Right)
        ctk.CTkLabel(inputs_row, text="Rechts:", font=ctk.CTkFont(size=11), text_color="#a1a1aa").pack(
            side="left", padx=(0, 2)
        )
        self.led_right = ctk.CTkEntry(inputs_row, width=44, height=28, justify="center")
        self.led_right.insert(0, str(config_data.get("leds_right", 9)))
        self.led_right.pack(side="left", padx=(0, 10))

        # Unten (Bottom)
        ctk.CTkLabel(inputs_row, text="Unten:", font=ctk.CTkFont(size=11), text_color="#a1a1aa").pack(
            side="left", padx=(0, 2)
        )
        self.led_bottom = ctk.CTkEntry(inputs_row, width=44, height=28, justify="center")
        self.led_bottom.insert(0, str(config_data.get("leds_bottom", 15)))
        self.led_bottom.pack(side="left")

        # Total LEDs Counter
        self.total_label = ctk.CTkLabel(
            self.led_box,
            text=self._calc_total_text(),
            font=ctk.CTkFont(size=11, weight="bold"),
            text_color="#10b981",
        )
        self.total_label.pack(side="bottom", anchor="e", padx=10, pady=(0, 6))

        for entry in [self.led_left, self.led_top, self.led_right, self.led_bottom]:
            entry.bind("<KeyRelease>", lambda e: self.update_total_label())

        # Edge Routing / Order
        self.order_frame = ctk.CTkFrame(self, fg_color="transparent")
        self.order_frame.pack(fill="x", padx=14, pady=6)

        ctk.CTkLabel(
            self.order_frame,
            text="LED-Reihenfolge (Kantenabfolge):",
            font=ctk.CTkFont(size=12, weight="bold"),
            text_color="#e4e4e7",
        ).pack(anchor="w")

        self.current_order: list[str] = list(
            config_data.get("edge_order", ["left", "top", "right", "bottom"])
        )

        preset_row = ctk.CTkFrame(self.order_frame, fg_color="transparent")
        preset_row.pack(fill="x", pady=(4, 6))

        ctk.CTkButton(
            preset_row,
            text="Uhrzeigersinn (L→O→R→U)",
            height=26,
            fg_color="#3f3f46",
            hover_color="#52525b",
            font=ctk.CTkFont(size=11),
            command=lambda: self.set_order(["left", "top", "right", "bottom"]),
        ).pack(side="left", padx=(0, 4))

        ctk.CTkButton(
            preset_row,
            text="Gegen Uhrzeigersinn (R→O→L→U)",
            height=26,
            fg_color="#3f3f46",
            hover_color="#52525b",
            font=ctk.CTkFont(size=11),
            command=lambda: self.set_order(["right", "top", "left", "bottom"]),
        ).pack(side="left")

        # Interactive Order Buttons
        btn_row = ctk.CTkFrame(self.order_frame, fg_color="transparent")
        btn_row.pack(fill="x", pady=2)

        self.order_buttons: dict[str, ctk.CTkButton] = {}
        for edge in ["left", "top", "right", "bottom"]:
            label_map = {"left": "LINKS", "top": "OBEN", "right": "RECHTS", "bottom": "UNTEN"}
            btn = ctk.CTkButton(
                btn_row,
                text=label_map[edge],
                width=64,
                height=26,
                font=ctk.CTkFont(size=11),
                command=lambda e=edge: self.add_edge_to_order(e),
            )
            btn.pack(side="left", padx=(0, 4))
            self.order_buttons[edge] = btn

        self.reset_btn = ctk.CTkButton(
            btn_row,
            text="Reset",
            width=50,
            height=26,
            fg_color="#52525b",
            hover_color="#71717a",
            font=ctk.CTkFont(size=11),
            command=self.reset_order,
        )
        self.reset_btn.pack(side="left", padx=(4, 0))

        # Display of active sequence
        self.order_disp = ctk.CTkLabel(
            self.order_frame,
            text="",
            font=ctk.CTkFont(size=12, weight="bold"),
            text_color="#06b6d4",
        )
        self.order_disp.pack(anchor="w", pady=(4, 2))
        self._refresh_order_display()

        # Reverse Direction Checkboxes
        self.dir_frame = ctk.CTkFrame(self, fg_color="transparent")
        self.dir_frame.pack(fill="x", padx=14, pady=(4, 12))

        ctk.CTkLabel(
            self.dir_frame,
            text="Richtung umkehren (Invertieren):",
            font=ctk.CTkFont(size=12),
            text_color="#a1a1aa",
        ).pack(anchor="w", pady=(0, 4))

        dirs = config_data.get("edge_directions", {"left": -1, "top": 1, "right": 1, "bottom": -1})
        self.dir_vars: dict[str, ctk.BooleanVar] = {}

        cb_row = ctk.CTkFrame(self.dir_frame, fg_color="transparent")
        cb_row.pack(fill="x")

        labels = {"left": "Links", "top": "Oben", "right": "Rechts", "bottom": "Unten"}
        for edge in ["left", "top", "right", "bottom"]:
            var = ctk.BooleanVar(value=(dirs.get(edge, 1) == -1))
            cb = ctk.CTkCheckBox(
                cb_row,
                text=labels[edge],
                variable=var,
                font=ctk.CTkFont(size=11),
                checkbox_width=18,
                checkbox_height=18,
            )
            cb.pack(side="left", padx=(0, 10))
            self.dir_vars[edge] = var

    def _calc_total_text(self) -> str:
        try:
            total = (
                int(self.led_left.get().strip() or 0)
                + int(self.led_top.get().strip() or 0)
                + int(self.led_right.get().strip() or 0)
                + int(self.led_bottom.get().strip() or 0)
            )
            return f"Gesamt: {total} LEDs"
        except Exception:
            return "Gesamt: 0 LEDs"

    def update_total_label(self):
        self.total_label.configure(text=self._calc_total_text())

    def add_edge_to_order(self, edge: str):
        if edge not in self.current_order:
            self.current_order.append(edge)
            self._refresh_order_display()

    def set_order(self, order: list[str]):
        self.current_order = list(order)
        self._refresh_order_display()

    def reset_order(self):
        self.current_order = []
        self._refresh_order_display()

    def _refresh_order_display(self):
        label_map = {"left": "LINKS", "top": "OBEN", "right": "RECHTS", "bottom": "UNTEN"}
        if self.current_order:
            txt = " ➔ ".join([label_map.get(e, e.upper()) for e in self.current_order])
        else:
            txt = "(Reihenfolge leer – Kanten anklicken)"
        self.order_disp.configure(text=txt)

        for edge, btn in self.order_buttons.items():
            if edge in self.current_order:
                btn.configure(fg_color="#059669", hover_color="#047857", state="disabled")
            else:
                btn.configure(fg_color="#3b82f6", hover_color="#2563eb", state="normal")

    def on_remove(self):
        self.remove_callback(self)

    def get_config(self) -> dict[str, Any]:
        try:
            idx = int(self.idx_entry.get().strip())
        except ValueError:
            idx = 1
        try:
            led_l = int(self.led_left.get().strip())
            led_t = int(self.led_top.get().strip())
            led_r = int(self.led_right.get().strip())
            led_b = int(self.led_bottom.get().strip())
        except ValueError:
            led_l, led_t, led_r, led_b = 9, 15, 9, 15

        return {
            "mss_index": idx,
            "leds_left": led_l,
            "leds_top": led_t,
            "leds_right": led_r,
            "leds_bottom": led_b,
            "edge_order": list(self.current_order) if self.current_order else ["left", "top", "right", "bottom"],
            "edge_directions": {edge: (-1 if var.get() else 1) for edge, var in self.dir_vars.items()},
        }


class GloWledApp(ctk.CTk if _GUI_AVAILABLE else object):  # type: ignore[misc]
    """
    Main Application Window for GloWled with intuitive multi-tab interface,
    live visual status, connection diagnostics, and smooth background streaming.
    """

    def __init__(
        self,
        first_start_mode: bool = False,
        start_minimized_arg: bool = False,
        sync_info: dict[str, Any] | None = None,
    ):
        if not _GUI_AVAILABLE:
            return
        super().__init__()

        self.title("GloWled — High-Performance Ambilight Controller")
        self.geometry("620x780")
        self.minsize(580, 700)
        self.configure(fg_color="#0f0f11")
        self.sync_info = sync_info or {}

        icon_path = resource_path("icon.ico")
        if os.path.exists(icon_path):
            try:
                self.iconbitmap(icon_path)
            except Exception:
                pass

        self.protocol("WM_DELETE_WINDOW", self.hide_window)

        self.app_config = load_config()
        self.worker: WLEDWorker | None = None
        self.tray_icon: pystray.Icon | None = None
        self.monitor_cards: list[MonitorCard] = []

        # Top Header Banner
        self._build_header()

        # Main Navigation Tabs
        self.tabview = ctk.CTkTabview(
            self,
            fg_color="#141417",
            segmented_button_fg_color="#202024",
            segmented_button_selected_color="#2563eb",
            segmented_button_selected_hover_color="#1d4ed8",
            corner_radius=10,
        )
        self.tabview.pack(fill="both", expand=True, padx=18, pady=(0, 10))

        self.tab_dash = self.tabview.add("⚡ Dashboard")
        self.tab_network = self.tabview.add("⚙️ WLED & Netzwerk")
        self.tab_effects = self.tabview.add("🎨 Bild & Effekte")
        self.tab_monitors = self.tabview.add("🖥️ Monitore")
        self.tab_info = self.tabview.add("ℹ️ Info")

        # Build Tab Content
        self._build_dashboard_tab()
        self._build_network_tab()
        self._build_effects_tab()
        self._build_monitors_tab()
        self._build_info_tab()

        # Bottom Action Bar
        self._build_bottom_bar()

        # Start State handling:
        # If launched with --autostart or --minimized, or config start_minimized is True:
        # run silently in tray. Otherwise, present the window for the user!
        should_minimize = start_minimized_arg or bool(self.app_config.get("start_minimized", False))

        if should_minimize:
            self.withdraw()
            self.start_worker_silent()
        else:
            self.deiconify()

        # Dynamic notification on first start or after moving the executable
        if first_start_mode:
            self.status_badge.configure(text="● ERSTSTART", fg_color="#065f46", text_color="#34d399")
            self.dash_status_label.configure(
                text="Willkommen bei GloWled! Automatische Einrichtung aktiv (Autostart & Verknüpfungen wurden eingerichtet)."
            )
        elif self.sync_info.get("path_migrated"):
            self.status_badge.configure(text="● PFAD AKTUALISIERT", fg_color="#1e3a8a", text_color="#60a5fa")
            self.dash_status_label.configure(
                text=f"Neuer Programm-Pfad erkannt und synchronisiert: {self.sync_info.get('current_path')}"
            )

    def _build_header(self):
        header_frame = ctk.CTkFrame(self, fg_color="transparent")
        header_frame.pack(fill="x", padx=20, pady=(14, 8))

        brand_frame = ctk.CTkFrame(header_frame, fg_color="transparent")
        brand_frame.pack(side="left")

        ctk.CTkLabel(
            brand_frame,
            text="GloWled",
            font=ctk.CTkFont(size=22, weight="bold"),
            text_color="#38bdf8",
        ).pack(side="left")

        ctk.CTkLabel(
            brand_frame,
            text=" • Ambilight Sync",
            font=ctk.CTkFont(size=14),
            text_color="#71717a",
        ).pack(side="left", padx=(4, 0))

        # Status Badge Indicator
        self.status_badge = ctk.CTkLabel(
            header_frame,
            text="● BEREIT",
            font=ctk.CTkFont(size=12, weight="bold"),
            text_color="#a1a1aa",
            fg_color="#27272a",
            corner_radius=6,
            padx=10,
            pady=4,
        )
        self.status_badge.pack(side="right")

    def _build_dashboard_tab(self):
        # Quick Status Card
        card = ctk.CTkFrame(self.tab_dash, fg_color="#18181b", corner_radius=10, border_width=1, border_color="#27272a")
        card.pack(fill="x", padx=12, pady=10)

        ctk.CTkLabel(card, text="Aktueller Status", font=ctk.CTkFont(size=14, weight="bold"), text_color="#f4f4f5").pack(
            anchor="w", padx=16, pady=(12, 4)
        )

        self.dash_status_text = ctk.CTkLabel(
            card,
            text="Streaming gestoppt. Klicken Sie auf 'Starten', um die Synchronisation zu beginnen.",
            font=ctk.CTkFont(size=12),
            text_color="#a1a1aa",
            wraplength=520,
            justify="left",
        )
        self.dash_status_text.pack(anchor="w", padx=16, pady=(0, 10))

        # Info metrics row
        metrics_row = ctk.CTkFrame(card, fg_color="transparent")
        metrics_row.pack(fill="x", padx=16, pady=(0, 12))

        self.dash_target_label = ctk.CTkLabel(
            metrics_row,
            text=f"Ziel: {self.app_config.get('ip', '127.0.0.1')}:{self.app_config.get('port', 21324)}",
            font=ctk.CTkFont(size=11),
            text_color="#38bdf8",
        )
        self.dash_target_label.pack(side="left", padx=(0, 16))

        self.dash_fps_label = ctk.CTkLabel(
            metrics_row,
            text=f"Rate: {self.app_config.get('fps', 30)} FPS",
            font=ctk.CTkFont(size=11),
            text_color="#34d399",
        )
        self.dash_fps_label.pack(side="left", padx=(0, 16))

        self.dash_leds_label = ctk.CTkLabel(
            metrics_row,
            text=f"Monitore: {len(self.app_config.get('monitors', []))}",
            font=ctk.CTkFont(size=11),
            text_color="#fbbf24",
        )
        self.dash_leds_label.pack(side="left")

        # Connection Test Box
        test_box = ctk.CTkFrame(self.tab_dash, fg_color="#18181b", corner_radius=10, border_width=1, border_color="#27272a")
        test_box.pack(fill="x", padx=12, pady=8)

        test_row = ctk.CTkFrame(test_box, fg_color="transparent")
        test_row.pack(fill="x", padx=16, pady=12)

        ctk.CTkLabel(
            test_row,
            text="WLED-Verbindung prüfen:",
            font=ctk.CTkFont(size=12, weight="bold"),
            text_color="#e4e4e7",
        ).pack(side="left")

        self.test_conn_btn = ctk.CTkButton(
            test_row,
            text="Ping testen",
            width=100,
            height=28,
            fg_color="#3b82f6",
            hover_color="#2563eb",
            command=self.test_connection_async,
        )
        self.test_conn_btn.pack(side="right")

        self.conn_result_label = ctk.CTkLabel(
            test_box,
            text="",
            font=ctk.CTkFont(size=11),
            text_color="#a1a1aa",
        )
        self.conn_result_label.pack(anchor="w", padx=16, pady=(0, 8))

        # Quick Tips / Shortcuts Card
        tips_card = ctk.CTkFrame(self.tab_dash, fg_color="#18181b", corner_radius=10, border_width=1, border_color="#27272a")
        tips_card.pack(fill="both", expand=True, padx=12, pady=8)

        ctk.CTkLabel(
            tips_card,
            text="💡 Schnelleinstieg & Tipps",
            font=ctk.CTkFont(size=13, weight="bold"),
            text_color="#f4f4f5",
        ).pack(anchor="w", padx=16, pady=(12, 6))

        tips_text = (
            "• Minimieren ins Tray: Schließen Sie das Fenster einfach mit [X], GloWled läuft unbemerkt im Hintergrund weiter.\n"
            "• Eco-Modus aktiv: Wenn der Bildschirminhalt stillsteht (z. B. beim Lesen oder Pausieren), sinkt die CPU-Auslastung auf fast 0%.\n"
            "• WLED Konfiguration: Stellen Sie sicher, dass in WLED unter 'Sync Interfaces' die Realtime-Schnittstelle (UDP Port 21324) aktiv ist.\n"
            "• Multi-Monitor: Im Reiter 'Monitore' können Sie beliebige Displays hinzufügen und die LED-Anordnung spiegeln oder invertieren."
        )
        ctk.CTkLabel(
            tips_card,
            text=tips_text,
            font=ctk.CTkFont(size=11),
            text_color="#a1a1aa",
            justify="left",
        ).pack(anchor="w", padx=16, pady=(0, 12))

    def _build_network_tab(self):
        card = ctk.CTkFrame(self.tab_network, fg_color="#18181b", corner_radius=10, border_width=1, border_color="#27272a")
        card.pack(fill="x", padx=12, pady=10)

        # WLED IP & Port
        row1 = ctk.CTkFrame(card, fg_color="transparent")
        row1.pack(fill="x", padx=16, pady=14)

        ctk.CTkLabel(row1, text="WLED IP-Adresse:", font=ctk.CTkFont(size=12), text_color="#e4e4e7", width=120, anchor="w").pack(
            side="left"
        )
        self.ip_entry = ctk.CTkEntry(row1, width=150, height=30)
        self.ip_entry.insert(0, str(self.app_config.get("ip", "127.0.0.1")))
        self.ip_entry.pack(side="left", padx=(0, 20))

        ctk.CTkLabel(row1, text="UDP Port:", font=ctk.CTkFont(size=12), text_color="#e4e4e7", width=70, anchor="w").pack(
            side="left"
        )
        self.port_entry = ctk.CTkEntry(row1, width=80, height=30)
        self.port_entry.insert(0, str(self.app_config.get("port", 21324)))
        self.port_entry.pack(side="left")

        # FPS Slider with Presets
        row_fps = ctk.CTkFrame(card, fg_color="transparent")
        row_fps.pack(fill="x", padx=16, pady=(0, 14))

        self.fps_label = ctk.CTkLabel(
            row_fps,
            text=f"Bildwiederholrate: {self.app_config.get('fps', 30)} FPS",
            font=ctk.CTkFont(size=12),
            text_color="#e4e4e7",
            width=160,
            anchor="w",
        )
        self.fps_label.pack(side="left")

        self.fps_slider = ctk.CTkSlider(
            row_fps,
            from_=15,
            to=60,
            number_of_steps=45,
            command=self.on_fps_change,
        )
        self.fps_slider.set(self.app_config.get("fps", 30))
        self.fps_slider.pack(side="left", fill="x", expand=True, padx=(10, 0))

        # Quick FPS Buttons
        preset_frame = ctk.CTkFrame(card, fg_color="transparent")
        preset_frame.pack(fill="x", padx=16, pady=(0, 14))

        ctk.CTkButton(
            preset_frame,
            text="Eco (30 FPS)",
            width=90,
            height=26,
            fg_color="#27272a",
            hover_color="#3f3f46",
            font=ctk.CTkFont(size=11),
            command=lambda: self._set_fps(30),
        ).pack(side="left", padx=(0, 6))

        ctk.CTkButton(
            preset_frame,
            text="Glatt (45 FPS)",
            width=90,
            height=26,
            fg_color="#27272a",
            hover_color="#3f3f46",
            font=ctk.CTkFont(size=11),
            command=lambda: self._set_fps(45),
        ).pack(side="left", padx=(0, 6))

        ctk.CTkButton(
            preset_frame,
            text="Ultra (60 FPS)",
            width=90,
            height=26,
            fg_color="#27272a",
            hover_color="#3f3f46",
            font=ctk.CTkFont(size=11),
            command=lambda: self._set_fps(60),
        ).pack(side="left")

        # Switches / Checkboxes Card
        switches_card = ctk.CTkFrame(self.tab_network, fg_color="#18181b", corner_radius=10, border_width=1, border_color="#27272a")
        switches_card.pack(fill="x", padx=12, pady=6)

        # Eco Mode Switch
        self.eco_var = ctk.BooleanVar(value=bool(self.app_config.get("eco_mode", True)))
        self.eco_switch = ctk.CTkSwitch(
            switches_card,
            text="Eco-Modus (CPU-Spitzen reduzieren bei Standbildern)",
            variable=self.eco_var,
            font=ctk.CTkFont(size=12),
        )
        self.eco_switch.pack(anchor="w", padx=16, pady=(14, 8))

        # Windows Autostart Checkbox
        self.autostart_var = ctk.BooleanVar(value=check_autostart())
        self.autostart_cb = ctk.CTkSwitch(
            switches_card,
            text="Beim Windows-Start automatisch im Hintergrund ausführen",
            variable=self.autostart_var,
            font=ctk.CTkFont(size=12),
            command=self.on_autostart_toggle,
        )
        self.autostart_cb.pack(anchor="w", padx=16, pady=(0, 8))

        # Start Minimized Checkbox
        self.minimized_var = ctk.BooleanVar(value=bool(self.app_config.get("start_minimized", False)))
        self.minimized_cb = ctk.CTkSwitch(
            switches_card,
            text="Immer direkt minimiert im System-Tray starten",
            variable=self.minimized_var,
            font=ctk.CTkFont(size=12),
        )
        self.minimized_cb.pack(anchor="w", padx=16, pady=(0, 14))

    def _set_fps(self, fps: int):
        self.fps_slider.set(fps)
        self.on_fps_change(fps)

    def on_fps_change(self, value):
        fps = int(value)
        self.fps_label.configure(text=f"Bildwiederholrate: {fps} FPS")
        self.dash_fps_label.configure(text=f"Rate: {fps} FPS")
        if self.worker and self.worker.is_running:
            self.worker.fps = fps

    def _build_effects_tab(self):
        card = ctk.CTkFrame(self.tab_effects, fg_color="#18181b", corner_radius=10, border_width=1, border_color="#27272a")
        card.pack(fill="x", padx=12, pady=10)

        # Brightness Slider
        row_bri = ctk.CTkFrame(card, fg_color="transparent")
        row_bri.pack(fill="x", padx=16, pady=12)

        self.bri_label = ctk.CTkLabel(
            row_bri,
            text=f"Helligkeit: {self.app_config.get('brightness', 1.0):.1f}x",
            font=ctk.CTkFont(size=12),
            width=130,
            anchor="w",
        )
        self.bri_label.pack(side="left")

        self.bri_slider = ctk.CTkSlider(
            row_bri,
            from_=0.1,
            to=3.0,
            number_of_steps=29,
            command=lambda v: self.bri_label.configure(text=f"Helligkeit: {float(v):.1f}x"),
        )
        self.bri_slider.set(self.app_config.get("brightness", 1.0))
        self.bri_slider.pack(side="left", fill="x", expand=True, padx=(10, 0))

        # Saturation Slider
        row_sat = ctk.CTkFrame(card, fg_color="transparent")
        row_sat.pack(fill="x", padx=16, pady=12)

        self.sat_label = ctk.CTkLabel(
            row_sat,
            text=f"Sättigung: {self.app_config.get('saturation', 1.0):.1f}x",
            font=ctk.CTkFont(size=12),
            width=130,
            anchor="w",
        )
        self.sat_label.pack(side="left")

        self.sat_slider = ctk.CTkSlider(
            row_sat,
            from_=0.0,
            to=3.0,
            number_of_steps=30,
            command=lambda v: self.sat_label.configure(text=f"Sättigung: {float(v):.1f}x"),
        )
        self.sat_slider.set(self.app_config.get("saturation", 1.0))
        self.sat_slider.pack(side="left", fill="x", expand=True, padx=(10, 0))

        # Transition Smoothing Filter Slider
        row_smooth = ctk.CTkFrame(card, fg_color="transparent")
        row_smooth.pack(fill="x", padx=16, pady=12)

        smooth_val = float(self.app_config.get("smoothing", 0.1))
        self.smooth_label = ctk.CTkLabel(
            row_smooth,
            text=f"Farb-Glättung: {int(smooth_val * 100)}%",
            font=ctk.CTkFont(size=12),
            width=130,
            anchor="w",
        )
        self.smooth_label.pack(side="left")

        self.smooth_slider = ctk.CTkSlider(
            row_smooth,
            from_=0.0,
            to=0.85,
            number_of_steps=17,
            command=lambda v: self.smooth_label.configure(text=f"Farb-Glättung: {int(float(v) * 100)}%"),
        )
        self.smooth_slider.set(smooth_val)
        self.smooth_slider.pack(side="left", fill="x", expand=True, padx=(10, 0))

        # Black Level Cutoff Threshold Slider
        row_black = ctk.CTkFrame(card, fg_color="transparent")
        row_black.pack(fill="x", padx=16, pady=12)

        black_thresh_val = int(self.app_config.get("black_threshold", 15))
        self.black_thresh_label = ctk.CTkLabel(
            row_black,
            text=f"Schwarz-Schwelle: {black_thresh_val}",
            font=ctk.CTkFont(size=12),
            width=130,
            anchor="w",
        )
        self.black_thresh_label.pack(side="left")

        self.black_thresh_slider = ctk.CTkSlider(
            row_black,
            from_=0,
            to=50,
            number_of_steps=50,
            command=lambda v: self.black_thresh_label.configure(text=f"Schwarz-Schwelle: {int(float(v))}"),
        )
        self.black_thresh_slider.set(black_thresh_val)
        self.black_thresh_slider.pack(side="left", fill="x", expand=True, padx=(10, 0))

        # Edge Sampling Depth Slider (Letterbox Crop)
        row_depth = ctk.CTkFrame(card, fg_color="transparent")
        row_depth.pack(fill="x", padx=16, pady=12)

        depth_val = float(self.app_config.get("edge_depth", 0.08))
        self.depth_label = ctk.CTkLabel(
            row_depth,
            text=f"Randtiefe: {int(depth_val * 100)}%",
            font=ctk.CTkFont(size=12),
            width=130,
            anchor="w",
        )
        self.depth_label.pack(side="left")

        self.depth_slider = ctk.CTkSlider(
            row_depth,
            from_=0.04,
            to=0.25,
            number_of_steps=21,
            command=lambda v: self.depth_label.configure(text=f"Randtiefe: {int(float(v) * 100)}%"),
        )
        self.depth_slider.set(depth_val)
        self.depth_slider.pack(side="left", fill="x", expand=True, padx=(10, 0))

        # Resolution Dropdown
        row_res = ctk.CTkFrame(card, fg_color="transparent")
        row_res.pack(fill="x", padx=16, pady=(12, 16))

        ctk.CTkLabel(
            row_res,
            text="Abtast-Auflösung:",
            font=ctk.CTkFont(size=12),
            width=130,
            anchor="w",
        ).pack(side="left")

        options = list(RESOLUTION_PRESETS.keys())
        self.res_menu = ctk.CTkOptionMenu(row_res, values=options)
        cur_res = self.app_config.get("resolution", "Auto (Adaptiv)")
        if cur_res not in options:
            cur_res = "Auto (Adaptiv)"
        self.res_menu.set(cur_res)
        self.res_menu.pack(side="left", fill="x", expand=True, padx=(10, 0))

    def _build_monitors_tab(self):
        # Action Bar for Monitors: Detect + Add
        action_bar = ctk.CTkFrame(self.tab_monitors, fg_color="transparent")
        action_bar.pack(fill="x", padx=12, pady=(10, 6))

        self.detect_btn = ctk.CTkButton(
            action_bar,
            text="🔍 Monitore erkennen",
            height=30,
            fg_color="#27272a",
            hover_color="#3f3f46",
            font=ctk.CTkFont(size=12),
            command=self.on_detect_monitors,
        )
        self.detect_btn.pack(side="left", padx=(0, 8))

        self.add_mon_btn = ctk.CTkButton(
            action_bar,
            text="+ Monitor hinzufügen",
            height=30,
            fg_color="#2563eb",
            hover_color="#1d4ed8",
            font=ctk.CTkFont(size=12, weight="bold"),
            command=self.on_add_monitor,
        )
        self.add_mon_btn.pack(side="right")

        # Scrollable Frame for Monitor Cards
        self.scroll_frame = ctk.CTkScrollableFrame(
            self.tab_monitors,
            fg_color="transparent",
            scrollbar_button_color="#27272a",
            scrollbar_button_hover_color="#3f3f46",
        )
        self.scroll_frame.pack(fill="both", expand=True, padx=8, pady=4)

        for mon_cfg in self.app_config.get("monitors", []):
            self.add_monitor_card(mon_cfg)

    def _build_info_tab(self):
        card = ctk.CTkFrame(self.tab_info, fg_color="#18181b", corner_radius=10, border_width=1, border_color="#27272a")
        card.pack(fill="both", expand=True, padx=12, pady=10)

        ctk.CTkLabel(
            card,
            text="GloWled — Ambilight Controller",
            font=ctk.CTkFont(size=16, weight="bold"),
            text_color="#38bdf8",
        ).pack(anchor="w", padx=16, pady=(14, 4))

        info_text = (
            "GloWled ist eine hochperformante, ressourcenschonende Ambilight-Software für WLED.\n"
            "Sie erfasst den Bildschirminhalt in Echtzeit via GPU/DirectX Desktop Duplication\n"
            "und überträgt die Randfarben ohne spürbare Latenz per UDP an Ihren ESP/WLED-Controller.\n\n"
            "📦 Ausführung: Standalone Portable Exe (selbsteinrichtend & verschiebesicher)\n"
            f"📍 Programm-Pfad: {get_current_executable_path()}\n"
            f"⚙️ Konfigurations-Pfad: {get_config_file_path()}\n\n"
            "📌 WLED Einrichtung:\n"
            "1. Öffnen Sie die WLED-Weboberfläche im Browser.\n"
            "2. Gehen Sie auf 'Config' -> 'Sync Interfaces'.\n"
            "3. Stellen Sie sicher, dass 'Receive: Realtime' aktiv ist (UDP Port 21324).\n\n"
            "📄 Lizenz: GNU General Public License v3 (GPLv3)\n"
            "💻 Plattform: Windows Desktop (mit automatischem Cross-Platform Fallback)\n"
            "🌐 Projekt: https://github.com/ghostside-net/GloWled\n"
            "🏷️ Version: v1.3.0"
        )
        ctk.CTkLabel(
            card,
            text=info_text,
            font=ctk.CTkFont(size=12),
            text_color="#d4d4d8",
            justify="left",
        ).pack(anchor="w", padx=16, pady=4)

    def _build_bottom_bar(self):
        btn_frame = ctk.CTkFrame(self, fg_color="transparent")
        btn_frame.pack(fill="x", padx=18, pady=(4, 14))

        self.apply_btn = ctk.CTkButton(
            btn_frame,
            text="▶ STARTEN & ÜBERNEHMEN",
            fg_color="#059669",
            hover_color="#047857",
            font=ctk.CTkFont(size=13, weight="bold"),
            height=40,
            command=self.apply_worker,
        )
        self.apply_btn.pack(side="left", fill="x", expand=True, padx=(0, 6))

        self.stop_btn = ctk.CTkButton(
            btn_frame,
            text="⏹ ANHALTEN",
            fg_color="#dc2626",
            hover_color="#991b1b",
            font=ctk.CTkFont(size=13, weight="bold"),
            height=40,
            state="disabled",
            command=self.stop_worker,
        )
        self.stop_btn.pack(side="left", fill="x", expand=True, padx=(6, 6))

        self.hide_btn = ctk.CTkButton(
            btn_frame,
            text="Minimieren",
            fg_color="#27272a",
            hover_color="#3f3f46",
            font=ctk.CTkFont(size=13),
            width=90,
            height=40,
            command=self.hide_window,
        )
        self.hide_btn.pack(side="right")

    def test_connection_async(self):
        self.conn_result_label.configure(text="Prüfe Verbindung zu WLED...", text_color="#fbbf24")
        self.test_conn_btn.configure(state="disabled")

        def _do_test():
            ip = self.ip_entry.get().strip()
            try:
                port = int(self.port_entry.get().strip())
            except ValueError:
                port = 21324

            reachable = False
            msg = ""
            try:
                # Test UDP socket creation & dummy probe packet
                probe_sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
                probe_sock.settimeout(1.5)
                # DRGB live packet with 1 black pixel
                probe_sock.sendto(bytearray([2, 2, 0, 0, 0]), (ip, port))
                probe_sock.close()
                reachable = True
                msg = f"✓ UDP-Paket erfolgreich an {ip}:{port} gesendet!"
            except Exception as e:
                reachable = False
                msg = f"⚠ Fehler beim Senden an {ip}:{port}: {e}"

            def _update_ui():
                self.conn_result_label.configure(
                    text=msg,
                    text_color="#10b981" if reachable else "#ef4444",
                )
                self.test_conn_btn.configure(state="normal")

            self.after(0, _update_ui)

        threading.Thread(target=_do_test, daemon=True).start()

    def add_monitor_card(self, config_data: dict[str, Any] | None = None):
        if config_data is None:
            config_data = DEFAULT_CONFIG["monitors"][0].copy()
            config_data["mss_index"] = len(self.monitor_cards) + 1

        card = MonitorCard(self.scroll_frame, config_data, self.remove_monitor_card)
        card.pack(fill="x", pady=6)
        self.monitor_cards.append(card)
        self.dash_leds_label.configure(text=f"Monitore: {len(self.monitor_cards)}")

    def remove_monitor_card(self, card: MonitorCard):
        if card in self.monitor_cards:
            self.monitor_cards.remove(card)
            card.destroy()
            self.dash_leds_label.configure(text=f"Monitore: {len(self.monitor_cards)}")

    def on_add_monitor(self):
        self.add_monitor_card()

    def on_detect_monitors(self):
        displays = detect_displays()
        # Inform user of detected displays
        disp_info = ", ".join([f"{d['name']} ({d['resolution']})" for d in displays])
        self.conn_result_label.configure(
            text=f"Erkannte Displays: {disp_info}", text_color="#38bdf8"
        )

    def on_autostart_toggle(self):
        enabled = bool(self.autostart_var.get())
        toggle_autostart(enabled)
        self.app_config["autostart"] = enabled
        save_config(self.app_config)

    def gather_config(self) -> dict[str, Any]:
        ip = self.ip_entry.get().strip() or "127.0.0.1"
        try:
            port = int(self.port_entry.get().strip())
        except ValueError:
            port = 21324
        fps = int(self.fps_slider.get())
        brightness = float(self.bri_slider.get())
        saturation = float(self.sat_slider.get())
        smoothing = float(self.smooth_slider.get())
        black_threshold = (
            float(self.black_thresh_slider.get())
            if hasattr(self, "black_thresh_slider")
            else float(self.app_config.get("black_threshold", 15))
        )
        edge_depth = float(self.depth_slider.get())
        resolution = self.res_menu.get()
        eco_mode = bool(self.eco_var.get())
        start_minimized = bool(self.minimized_var.get())
        autostart = bool(self.autostart_var.get())
        last_known_exe_path = str(self.app_config.get("last_known_exe_path", get_current_executable_path()))
        oversample_factor = float(self.app_config.get("oversample_factor", 2.0))

        monitors = [c.get_config() for c in self.monitor_cards]
        if not monitors:
            monitors = [DEFAULT_CONFIG["monitors"][0].copy()]

        cfg = {
            "ip": ip,
            "port": port,
            "fps": fps,
            "brightness": brightness,
            "saturation": saturation,
            "smoothing": smoothing,
            "black_threshold": black_threshold,
            "edge_depth": edge_depth,
            "resolution": resolution,
            "oversample_factor": oversample_factor,
            "eco_mode": eco_mode,
            "start_minimized": start_minimized,
            "autostart": autostart,
            "last_known_exe_path": last_known_exe_path,
            "monitors": monitors,
        }
        save_config(cfg)
        self.app_config = cfg
        return cfg

    def apply_worker(self):
        config = self.gather_config()

        if self.worker is not None and self.worker.is_running:
            self.worker.stop()
            self.worker.join(timeout=1.0)

        self.worker = WLEDWorker(
            ip=config["ip"],
            port=config["port"],
            fps=config["fps"],
            monitor_configs=config["monitors"],
            brightness=config.get("brightness", 1.0),
            saturation=config.get("saturation", 1.0),
            resolution=config.get("resolution", "Auto (Adaptiv)"),
            smoothing=config.get("smoothing", 0.1),
            edge_depth=config.get("edge_depth", 0.08),
            eco_mode=config.get("eco_mode", True),
            black_threshold=config.get("black_threshold", 15),
            oversample_factor=config.get("oversample_factor", 2.0),
        )
        self.worker.start()

        # Update Status Visuals
        self.status_badge.configure(text="● STREAMING AKTIV", text_color="#10b981", fg_color="#064e3b")
        self.dash_status_text.configure(
            text=f"🟢 Ambilight aktiv! Bildschirminhalt wird live an {config['ip']}:{config['port']} gesendet.",
            text_color="#34d399",
        )
        self.dash_target_label.configure(text=f"Ziel: {config['ip']}:{config['port']}")
        self.apply_btn.configure(text="✔ EINSTELLUNGEN AKTUALISIERT")
        self.stop_btn.configure(state="normal")

    def stop_worker(self):
        if self.worker is not None and self.worker.is_running:
            self.worker.stop()
            self.worker.join(timeout=1.0)
            self.worker = None

        self.status_badge.configure(text="● GESTOPPT", text_color="#f87171", fg_color="#450a0a")
        self.dash_status_text.configure(
            text="Streaming angehalten. Klicken Sie auf 'Starten', um die Synchronisation fortzusetzen.",
            text_color="#a1a1aa",
        )
        self.apply_btn.configure(text="▶ STARTEN & ÜBERNEHMEN")
        self.stop_btn.configure(state="disabled")

    def start_worker_silent(self):
        if self.worker is None or not self.worker.is_running:
            config = self.app_config
            self.worker = WLEDWorker(
                ip=config["ip"],
                port=config["port"],
                fps=config["fps"],
                monitor_configs=config["monitors"],
                brightness=config.get("brightness", 1.0),
                saturation=config.get("saturation", 1.0),
                resolution=config.get("resolution", "Auto (Adaptiv)"),
                smoothing=config.get("smoothing", 0.1),
                edge_depth=config.get("edge_depth", 0.08),
                eco_mode=config.get("eco_mode", True),
                black_threshold=config.get("black_threshold", 15),
                oversample_factor=config.get("oversample_factor", 2.0),
            )
            self.worker.start()
            self.status_badge.configure(text="● STREAMING AKTIV", text_color="#10b981", fg_color="#064e3b")
            self.apply_btn.configure(text="✔ EINSTELLUNGEN AKTUALISIERT")
            self.stop_btn.configure(state="normal")
            self.worker.start()
            self.status_badge.configure(text="● STREAMING AKTIV", text_color="#10b981", fg_color="#064e3b")
            self.apply_btn.configure(text="✔ EINSTELLUNGEN AKTUALISIERT")
            self.stop_btn.configure(state="normal")

    def hide_window(self):
        """Minimize to tray instead of quitting."""
        if not os.path.exists(get_config_file_path()):
            # First run without save: ask or save config
            self.gather_config()
        self.withdraw()

    def show_window(self, icon=None, item=None):
        self.after(0, self.deiconify)

    def create_tray_icon(self):
        icon_path = resource_path("icon.ico")
        if os.path.exists(icon_path):
            try:
                image = Image.open(icon_path)
            except Exception:
                image = self._create_fallback_icon()
        else:
            image = self._create_fallback_icon()

        menu = pystray.Menu(
            pystray.MenuItem("GloWled öffnen", self.show_window, default=True),
            pystray.MenuItem(
                "Streaming starten",
                lambda: self.after(0, self.apply_worker),
            ),
            pystray.MenuItem(
                "Streaming anhalten",
                lambda: self.after(0, self.stop_worker),
            ),
            pystray.Menu.SEPARATOR,
            pystray.MenuItem("Beenden", self.quit_app),
        )

        self.tray_icon = pystray.Icon("GloWled", image, "GloWled Ambilight", menu)
        try:
            self.tray_icon.run()
        except Exception as e:
            print(f"[Tray] Tray icon terminated: {e}")

    def _create_fallback_icon(self):
        img = Image.new("RGB", (64, 64), color=(18, 18, 22))
        draw = ImageDraw.Draw(img)
        draw.ellipse([8, 8, 56, 56], fill=(56, 189, 248))
        draw.ellipse([20, 20, 44, 44], fill=(16, 185, 129))
        return img

    def quit_app(self, icon=None, item=None):
        if self.worker is not None:
            self.worker.stop()
            self.worker.join(timeout=1.0)
            self.worker = None

        if self.tray_icon is not None:
            try:
                self.tray_icon.stop()
            except Exception:
                pass

        try:
            self.destroy()
        except Exception:
            pass
        os._exit(0)


def run_headless_streaming():
    """Run streaming loop in headless CLI mode."""
    cfg = load_config()
    print("=" * 60)
    print("GloWled — Headless Ambient Lighting Service")
    print(f"Target: {cfg.get('ip')}:{cfg.get('port')} @ {cfg.get('fps')} FPS")
    print("Eco Mode:", cfg.get("eco_mode", True))
    print("=" * 60)

    worker = WLEDWorker(
        ip=cfg["ip"],
        port=cfg["port"],
        fps=cfg["fps"],
        monitor_configs=cfg["monitors"],
        brightness=cfg.get("brightness", 1.0),
        saturation=cfg.get("saturation", 1.0),
        resolution=cfg.get("resolution", "Auto (Adaptiv)"),
        smoothing=cfg.get("smoothing", 0.1),
        edge_depth=cfg.get("edge_depth", 0.08),
        eco_mode=cfg.get("eco_mode", True),
        black_threshold=cfg.get("black_threshold", 15),
        oversample_factor=cfg.get("oversample_factor", 2.0),
    )
    worker.start()
    print("Worker running. Press Ctrl+C to terminate.")
    try:
        while True:
            time.sleep(1.0)
    except KeyboardInterrupt:
        print("\nStopping worker...")
        worker.stop()
        worker.join(timeout=2.0)
        print("GloWled stopped.")


def main():
    parser = argparse.ArgumentParser(description="GloWled — High-Performance WLED Ambilight Controller")
    parser.add_argument("--headless", action="store_true", help="Run in background CLI streaming mode without GUI")
    parser.add_argument("--version", action="store_true", help="Print version and exit")
    parser.add_argument("--autostart", action="store_true", help="Run silently in system tray on system boot")
    parser.add_argument("--minimized", action="store_true", help="Start minimized to system tray")
    args = parser.parse_args()

    if args.version:
        print("GloWled v1.3.0")
        return

    # Single-instance enforcement (Windows)
    single_instance_lock = acquire_single_instance_lock()
    if not single_instance_lock:
        if args.autostart or args.minimized:
            # Background launch while already active: exit silently
            return
        bring_existing_instance_to_front()
        return

    # Automatic environment configuration, autostart, and folder migration check
    cfg = load_config()
    is_first_start = not os.path.exists(get_config_file_path())
    sync_info = sync_portable_environment(cfg)

    if args.headless or not _GUI_AVAILABLE:
        if not _GUI_AVAILABLE and not args.headless:
            print("[GloWled] GUI environment (customtkinter/X11) not detected, falling back to headless streaming mode.")
        run_headless_streaming()
        return

    start_min = bool(args.autostart or args.minimized)
    app = GloWledApp(
        first_start_mode=is_first_start,
        start_minimized_arg=start_min,
        sync_info=sync_info,
    )

    # Start system tray icon in a dedicated daemon thread
    threading.Thread(target=app.create_tray_icon, daemon=True).start()

    app.mainloop()


if __name__ == "__main__":
    main()
