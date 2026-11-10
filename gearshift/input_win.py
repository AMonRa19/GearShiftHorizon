from __future__ import annotations

import ctypes
import sys
import time

_KEY_UP = 0x45
_KEY_DOWN = 0x51
_KEYEVENTF_KEYUP = 0x0002


class KeyboardOutput:
    """Minimal Windows SendInput adapter for configured FH6 E/Q bindings."""

    def __init__(self, hold_ms: int = 40) -> None:
        self.hold_ms = max(10, min(120, int(hold_ms)))

    def shift(self, direction: str, count: int = 1, gap_ms: int = 60) -> None:
        if sys.platform != "win32":
            raise OSError("Automatic keyboard output is supported on Windows only")
        vk = _KEY_UP if direction == "UP" else _KEY_DOWN
        count = max(1, min(6, int(count)))
        user32 = ctypes.WinDLL("user32", use_last_error=True)

        class KEYBDINPUT(ctypes.Structure):
            _fields_ = [("wVk", ctypes.c_ushort), ("wScan", ctypes.c_ushort),
                        ("dwFlags", ctypes.c_ulong), ("time", ctypes.c_ulong),
                        ("dwExtraInfo", ctypes.POINTER(ctypes.c_ulong))]

        class INPUT_UNION(ctypes.Union):
            _fields_ = [("ki", KEYBDINPUT)]

        class INPUT(ctypes.Structure):
            _fields_ = [("type", ctypes.c_ulong), ("union", INPUT_UNION)]

        extra = ctypes.c_ulong(0)
        down = INPUT(type=1, union=INPUT_UNION(ki=KEYBDINPUT(vk, 0, 0, 0, ctypes.pointer(extra))))
        up = INPUT(type=1, union=INPUT_UNION(ki=KEYBDINPUT(vk, 0, _KEYEVENTF_KEYUP, 0, ctypes.pointer(extra))))
        for index in range(count):
            if user32.SendInput(1, ctypes.byref(down), ctypes.sizeof(INPUT)) != 1:
                raise ctypes.WinError(ctypes.get_last_error())
            time.sleep(self.hold_ms / 1000.0)
            if user32.SendInput(1, ctypes.byref(up), ctypes.sizeof(INPUT)) != 1:
                raise ctypes.WinError(ctypes.get_last_error())
            if index + 1 < count:
                time.sleep(max(20, min(150, int(gap_ms))) / 1000.0)
