"""Search a running process's memory for a string (ASCII and UTF-16LE) and dump context.

usage: python memscan.py <pid> <text | hex:0a0b...> [context_bytes]
"""
import ctypes
import ctypes.wintypes as wt
import sys

PROCESS_QUERY_INFORMATION = 0x0400
PROCESS_VM_READ = 0x0010
MEM_COMMIT = 0x1000
PAGE_NOACCESS = 0x01
PAGE_GUARD = 0x100


class MBI(ctypes.Structure):
    _fields_ = [
        ("BaseAddress", ctypes.c_void_p),
        ("AllocationBase", ctypes.c_void_p),
        ("AllocationProtect", wt.DWORD),
        ("PartitionId", wt.WORD),
        ("RegionSize", ctypes.c_size_t),
        ("State", wt.DWORD),
        ("Protect", wt.DWORD),
        ("Type", wt.DWORD),
    ]


k32 = ctypes.WinDLL("kernel32", use_last_error=True)
k32.OpenProcess.restype = wt.HANDLE
k32.VirtualQueryEx.argtypes = [wt.HANDLE, ctypes.c_void_p, ctypes.POINTER(MBI), ctypes.c_size_t]
k32.ReadProcessMemory.argtypes = [wt.HANDLE, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t)]


def regions(h):
    addr = 0
    mbi = MBI()
    while k32.VirtualQueryEx(h, ctypes.c_void_p(addr), ctypes.byref(mbi), ctypes.sizeof(mbi)):
        if mbi.State == MEM_COMMIT and not (mbi.Protect & (PAGE_NOACCESS | PAGE_GUARD)):
            yield mbi.BaseAddress, mbi.RegionSize
        addr = (mbi.BaseAddress or 0) + mbi.RegionSize


def read(h, addr, size):
    buf = ctypes.create_string_buffer(size)
    got = ctypes.c_size_t()
    if k32.ReadProcessMemory(h, ctypes.c_void_p(addr), buf, size, ctypes.byref(got)):
        return buf.raw[: got.value]
    return b""


def hexdump(data, base):
    for i in range(0, len(data), 16):
        chunk = data[i:i + 16]
        text = "".join(chr(b) if 32 <= b < 127 else "." for b in chunk)
        print(f"  {base + i:016x}  {chunk.hex(' '):<47}  {text}")


def main():
    pid, text = int(sys.argv[1]), sys.argv[2]
    ctx = int(sys.argv[3]) if len(sys.argv) > 3 else 64
    h = k32.OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, False, pid)
    if text.startswith("hex:"):
        needles = {"bytes": bytes.fromhex(text[4:])}
    else:
        needles = {"ascii": text.encode(), "utf16": text.encode("utf-16-le")}
    for base, size in regions(h):
        data = read(h, base, size)
        for kind, needle in needles.items():
            i = data.find(needle)
            while i != -1:
                print(f"{kind} hit at {base + i:#x} (region {base:#x}+{size:#x})")
                start = max(0, i - ctx)
                hexdump(data[start:i + len(needle) + ctx], base + start)
                i = data.find(needle, i + 1)


main()
