"""Development hot reload: copy a DLL under a fresh name and load it into a running process.
The shroudlink client addon shuts its previous copy down itself.

usage: python inject.py <pid|process name> <dll>
"""
import ctypes
import ctypes.wintypes as wt
import os
import shutil
import subprocess
import sys
import time

k32 = ctypes.WinDLL("kernel32", use_last_error=True)
k32.OpenProcess.restype = wt.HANDLE
k32.VirtualAllocEx.restype = ctypes.c_void_p
k32.VirtualAllocEx.argtypes = [wt.HANDLE, ctypes.c_void_p, ctypes.c_size_t, wt.DWORD, wt.DWORD]
k32.WriteProcessMemory.argtypes = [wt.HANDLE, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t)]
k32.GetProcAddress.restype = ctypes.c_void_p
k32.GetProcAddress.argtypes = [wt.HMODULE, ctypes.c_char_p]
k32.GetModuleHandleW.restype = wt.HMODULE
k32.CreateRemoteThread.restype = wt.HANDLE
k32.CreateRemoteThread.argtypes = [wt.HANDLE, ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_void_p, wt.DWORD, ctypes.c_void_p]


def find_pid(name):
    out = subprocess.run(["tasklist", "/FI", f"IMAGENAME eq {name}", "/FO", "CSV", "/NH"], capture_output=True, text=True).stdout
    for line in out.splitlines():
        parts = [p.strip('"') for p in line.split(",")]
        if len(parts) > 1 and parts[0].lower() == name.lower():
            return int(parts[1])
    raise SystemExit(f"{name} is not running")


def main():
    target, dll = sys.argv[1], os.path.abspath(sys.argv[2])
    pid = int(target) if target.isdigit() else find_pid(target)
    devdir = os.path.join(os.path.dirname(dll), "hotreload")
    os.makedirs(devdir, exist_ok=True)
    copy = os.path.join(devdir, f"{os.path.splitext(os.path.basename(dll))[0]}_{int(time.time())}.dll")
    shutil.copy2(dll, copy)
    path = (copy + "\0").encode("utf-16-le")
    h = k32.OpenProcess(0x1F0FFF, False, pid)
    if not h:
        raise SystemExit(f"OpenProcess failed: {ctypes.get_last_error()}")
    mem = k32.VirtualAllocEx(h, None, len(path), 0x3000, 0x04)
    k32.WriteProcessMemory(h, mem, path, len(path), None)
    load = k32.GetProcAddress(k32.GetModuleHandleW("kernel32.dll"), b"LoadLibraryW")
    t = k32.CreateRemoteThread(h, None, 0, load, mem, 0, None)
    k32.WaitForSingleObject(t, 10000)
    code = wt.DWORD()
    k32.GetExitCodeThread(t, ctypes.byref(code))
    print(f"injected {copy} into {pid}: module handle (low 32 bits) {code.value:#x}")


main()
