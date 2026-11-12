from __future__ import annotations

import ctypes
import sys
import time
from ctypes import wintypes

_KEY_UP = ord("E")
_KEY_DOWN = ord("Q")
_KEYEVENTF_KEYUP = 0x0002
ULONG_PTR = ctypes.c_size_t


class MOUSEINPUT(ctypes.Structure):
    _fields_ = [
        ("dx", wintypes.LONG),
        ("dy", wintypes.LONG),
        ("mouseData", wintypes.DWORD),
        ("dwFlags", wintypes.DWORD),
        ("time", wintypes.DWORD),
        ("dwExtraInfo", ULONG_PTR),
    ]


class KEYBDINPUT(ctypes.Structure):
    _fields_ = [
        ("wVk", wintypes.WORD),
        ("wScan", wintypes.WORD),
        ("dwFlags", wintypes.DWORD),
        ("time", wintypes.DWORD),
        ("dwExtraInfo", ULONG_PTR),
    ]


class HARDWAREINPUT(ctypes.Structure):
    _fields_ = [
        ("uMsg", wintypes.DWORD),
        ("wParamL", wintypes.WORD),
        ("wParamH", wintypes.WORD),
    ]


class INPUT_UNION(ctypes.Union):
    _fields_ = [
        ("mi", MOUSEINPUT),
        ("ki", KEYBDINPUT),
        ("hi", HARDWAREINPUT),
    ]


class INPUT(ctypes.Structure):
    _anonymous_ = ("union",)
    _fields_ = [("type", wintypes.DWORD), ("union", INPUT_UNION)]


class KeyboardOutput:
    """Windows SendInput adapter for FH6's configured E/Q shift bindings."""

    def __init__(self, hold_ms: int = 40) -> None:
        self.hold_ms = max(10, min(120, int(hold_ms)))

    @staticmethod
    def _user32():
        if sys.platform != "win32":
            raise OSError("Automatic keyboard output is supported on Windows only")
        user32 = ctypes.WinDLL("user32", use_last_error=True)
        user32.GetForegroundWindow.argtypes = []
        user32.GetForegroundWindow.restype = wintypes.HWND
        user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
        user32.GetWindowTextW.restype = ctypes.c_int
        user32.SendInput.argtypes = [wintypes.UINT, ctypes.POINTER(INPUT), ctypes.c_int]
        user32.SendInput.restype = wintypes.UINT
        return user32

    def shift(self, direction: str, count: int = 1, gap_ms: int = 60) -> None:
        if direction not in ("UP", "DOWN"):
            raise ValueError(f"Unsupported shift direction: {direction}")

        user32 = self._user32()
        foreground = user32.GetForegroundWindow()
        if not foreground:
            raise OSError("Could not identify the active Windows window")

        title = ctypes.create_unicode_buffer(512)
        user32.GetWindowTextW(foreground, title, len(title))
        if "forza" not in title.value.lower():
            raise OSError(
                f"Forza must be the active foreground window (current: {title.value or 'untitled'})"
            )

        vk = _KEY_UP if direction == "UP" else _KEY_DOWN
        count = max(1, min(10, int(count)))
        down = INPUT(type=1, ki=KEYBDINPUT(vk, 0, 0, 0, 0))
        up = INPUT(type=1, ki=KEYBDINPUT(vk, 0, _KEYEVENTF_KEYUP, 0, 0))

        for index in range(count):
            ctypes.set_last_error(0)
            sent = user32.SendInput(1, ctypes.byref(down), ctypes.sizeof(INPUT))
            if sent != 1:
                raise ctypes.WinError(ctypes.get_last_error())

            time.sleep(self.hold_ms / 1000.0)

            ctypes.set_last_error(0)
            sent = user32.SendInput(1, ctypes.byref(up), ctypes.sizeof(INPUT))
            if sent != 1:
                raise ctypes.WinError(ctypes.get_last_error())

            if index + 1 < count:
                time.sleep(max(20, min(150, int(gap_ms))) / 1000.0)
