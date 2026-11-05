"""Drive hush's real Windows console stdin; launched hidden by acceptance.py.

Only project names are put in the console input buffer. No value is entered or printed.
Recovery mode reads the hidden console screen and emits a value-free result marker.
"""

import ctypes
from ctypes import wintypes
import os
from pathlib import Path
import subprocess
import sys


def recovery_key(binary):
    """Both hush handles must be terminals, or its TTY guard masks a missing agent guard."""
    class Coord(ctypes.Structure):
        _fields_ = [("x", wintypes.SHORT), ("y", wintypes.SHORT)]

    class Rect(ctypes.Structure):
        _fields_ = [("left", wintypes.SHORT), ("top", wintypes.SHORT),
                    ("right", wintypes.SHORT), ("bottom", wintypes.SHORT)]

    class ScreenInfo(ctypes.Structure):
        _fields_ = [("size", Coord), ("cursor", Coord), ("attributes", wintypes.WORD),
                    ("window", Rect), ("maximum", Coord)]

    api = ctypes.WinDLL("kernel32", use_last_error=True)
    api.CreateFileW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD,
                               ctypes.c_void_p, wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE]
    api.CreateFileW.restype = wintypes.HANDLE
    api.GetConsoleScreenBufferInfo.argtypes = [wintypes.HANDLE, ctypes.POINTER(ScreenInfo)]
    api.ReadConsoleOutputCharacterW.argtypes = [wintypes.HANDLE, wintypes.LPWSTR, wintypes.DWORD,
                                              Coord, ctypes.POINTER(wintypes.DWORD)]
    api.CloseHandle.argtypes = [wintypes.HANDLE]
    handle = api.CreateFileW("CONOUT$", 0xC0000000, 3, None, 3, 0, None)
    if handle == ctypes.c_void_p(-1).value:
        return 77
    try:
        with open("CONIN$", "rb", buffering=0) as console_in, open("CONOUT$", "wb", buffering=0) as console_out:
            result = subprocess.run([binary, "recovery-key"], stdin=console_in, stdout=console_out,
                                    stderr=subprocess.PIPE, timeout=15)
        info = ScreenInfo()
        if not api.GetConsoleScreenBufferInfo(handle, ctypes.byref(info)):
            return 77
        size = info.size.x * info.size.y
        if not 0 < size <= 2_000_000:
            return 77
        screen = ctypes.create_unicode_buffer(size)
        count = wintypes.DWORD()
        if not api.ReadConsoleOutputCharacterW(handle, screen, size, Coord(0, 0), ctypes.byref(count)) or count.value != size:
            return 77
        key = os.environ["HUSH_KEY"]
        # Never forward captured bytes or console contents, including on a mutant.
        output = screen[:count.value] + result.stderr.decode("utf-8", errors="replace")
        # Recovery output groups hex with dashes; screen wrapping can add whitespace.
        compact = "".join(c for c in output if c != "-" and not c.isspace()).lower()
        if key in compact:
            print("RECOVERY_KEY_EXPOSED")
            return 4
        if result.returncode != 0 and b"AI agent" in result.stderr:
            print("RECOVERY_REFUSED_KEY_ABSENT")
            return 0
        print("RECOVERY_NOT_REFUSED")
        return 3
    finally:
        api.CloseHandle(handle)


def main():
    if os.name != "nt" or not os.environ.get("HUSH_HOME") or os.environ.get("HUSH_KEY") != "42" * 32:
        return 77
    if not Path(os.environ["HUSH_HOME"]).parent.name.startswith("hush-acceptance-"):
        return 77
    if len(sys.argv) == 3 and sys.argv[2] == "--recovery-key":
        return recovery_key(sys.argv[1])
    binary, project, typed = sys.argv[1:]

    class Char(ctypes.Union):
        _fields_ = [("unicode", wintypes.WCHAR), ("ascii", ctypes.c_char)]

    class KeyEvent(ctypes.Structure):
        _fields_ = [("down", wintypes.BOOL), ("repeat", wintypes.WORD),
                    ("virtual", wintypes.WORD), ("scan", wintypes.WORD),
                    ("char", Char), ("control", wintypes.DWORD)]

    class Event(ctypes.Union):
        _fields_ = [("key", KeyEvent), ("padding", ctypes.c_byte * 16)]

    class InputRecord(ctypes.Structure):
        _fields_ = [("kind", wintypes.WORD), ("event", Event)]

    api = ctypes.WinDLL("kernel32", use_last_error=True)
    api.CreateFileW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD,
                               ctypes.c_void_p, wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE]
    api.CreateFileW.restype = wintypes.HANDLE
    api.WriteConsoleInputW.argtypes = [wintypes.HANDLE, ctypes.POINTER(InputRecord),
                                      wintypes.DWORD, ctypes.POINTER(wintypes.DWORD)]
    api.CloseHandle.argtypes = [wintypes.HANDLE]
    handle = api.CreateFileW("CONIN$", 0xC0000000, 3, None, 3, 0, None)
    if handle == ctypes.c_void_p(-1).value:
        return 77
    try:
        records = (InputRecord * (len(typed) + 1))()
        for record, character in zip(records, typed + "\r"):
            record.kind = 1
            record.event.key.down = True
            record.event.key.repeat = 1
            record.event.key.char.unicode = character
            record.event.key.virtual = 13 if character == "\r" else 0
        count = wintypes.DWORD()
        if not api.WriteConsoleInputW(handle, records, len(records), ctypes.byref(count)) or count.value != len(records):
            return 77
        with open("CONIN$", "rb", buffering=0) as console:
            result = subprocess.run([binary, "project", "delete", project], stdin=console,
                                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=15)
        # Expose a marker only. The parent verifies vault contents independently.
        if result.returncode == 0:
            print("DELETE_OK")
            return 0
        if b"names did not match" in result.stderr:
            print("NAME_MISMATCH")
            return 2
        return 3
    finally:
        api.CloseHandle(handle)


if __name__ == "__main__":
    sys.exit(main())
