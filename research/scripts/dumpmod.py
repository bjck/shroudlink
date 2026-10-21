"""Dump a module of a running process (e.g. the DRM-unpacked enshrouded.exe) to a PE file
that Ghidra can load: sections are laid out at their virtual addresses and ImageBase is set
to the live base, so absolute pointers in the dump stay valid.

usage: python dumpmod.py <pid> <module name> <out.exe>
"""
import ctypes
import ctypes.wintypes as wt
import struct
import sys

PROCESS_QUERY_INFORMATION = 0x0400
PROCESS_VM_READ = 0x0010

k32 = ctypes.WinDLL("kernel32", use_last_error=True)
psapi = ctypes.WinDLL("psapi", use_last_error=True)
k32.OpenProcess.restype = wt.HANDLE
k32.ReadProcessMemory.argtypes = [wt.HANDLE, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t)]
psapi.EnumProcessModulesEx.argtypes = [wt.HANDLE, ctypes.POINTER(wt.HMODULE), wt.DWORD, ctypes.POINTER(wt.DWORD), wt.DWORD]
psapi.GetModuleBaseNameW.argtypes = [wt.HANDLE, wt.HMODULE, wt.LPWSTR, wt.DWORD]


class ModuleInfo(ctypes.Structure):
    _fields_ = [("base", ctypes.c_void_p), ("size", wt.DWORD), ("entry", ctypes.c_void_p)]


psapi.GetModuleInformation.argtypes = [wt.HANDLE, wt.HMODULE, ctypes.POINTER(ModuleInfo), wt.DWORD]


def main():
    pid, name, out = int(sys.argv[1]), sys.argv[2].lower(), sys.argv[3]
    h = k32.OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, False, pid)
    mods = (wt.HMODULE * 1024)()
    needed = wt.DWORD()
    psapi.EnumProcessModulesEx(h, mods, ctypes.sizeof(mods), ctypes.byref(needed), 3)
    for m in mods[: needed.value // ctypes.sizeof(wt.HMODULE)]:
        buf = ctypes.create_unicode_buffer(260)
        psapi.GetModuleBaseNameW(h, m, buf, 260)
        if buf.value.lower() == name:
            info = ModuleInfo()
            psapi.GetModuleInformation(h, m, ctypes.byref(info), ctypes.sizeof(info))
            break
    else:
        raise SystemExit(f"{name} not loaded in {pid}")

    base, size = info.base, info.size
    image = bytearray(size)
    page = 0x1000
    unreadable = 0
    for off in range(0, size, page):
        chunk = (ctypes.c_char * page)()
        got = ctypes.c_size_t()
        if k32.ReadProcessMemory(h, ctypes.c_void_p(base + off), chunk, page, ctypes.byref(got)):
            image[off : off + page] = chunk.raw
        else:
            unreadable += 1

    pe = struct.unpack_from("<I", image, 0x3C)[0]
    nsec = struct.unpack_from("<H", image, pe + 6)[0]
    opt = pe + 24
    struct.pack_into("<Q", image, opt + 24, base)  # ImageBase (PE32+)
    sec = opt + struct.unpack_from("<H", image, pe + 20)[0]
    for i in range(nsec):
        s = sec + i * 40
        vsize, vaddr = struct.unpack_from("<II", image, s + 8)
        struct.pack_into("<II", image, s + 16, vsize, vaddr)  # SizeOfRawData, PointerToRawData
    with open(out, "wb") as f:
        f.write(image)
    print(f"dumped {name} base={base:#x} size={size:#x} unreadable pages={unreadable} -> {out}")


main()
