"""Drive hush's real Windows console stdin; launched hidden by acceptance.py.

Only project names are put in the console input buffer. No value is entered or printed.
"""

import ctypes
from ctypes import wintypes
import os
from pathlib import Path
import subprocess
import sys


def main():
    if os.name != "nt" or not os.environ.get("HUSH_HOME") or os.environ.get("HUSH_KEY") != "42" * 32:
        return 77
    binary, project, typed = sys.argv[1:]
    if not Path(os.environ["HUSH_HOME"]).parent.name.startswith("hush-acceptance-"):
        return 77

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
