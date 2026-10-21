"""Fast code references without Ghidra: finds RIP-relative lea/mov/cmp operands in .text
that point at the given addresses (or strings) and maps each hit to its function via .pdata.

usage: python ripxref.py <dump.exe> <target> [...]
  target: 0x<address> or a string (located in the image, first exact NUL-terminated match)
"""
import bisect
import re
import struct
import sys

import pefile


def main():
    path, targets = sys.argv[1], sys.argv[2:]
    pe = pefile.PE(path, fast_load=True)
    base = pe.OPTIONAL_HEADER.ImageBase
    data = open(path, "rb").read()  # dump: file offset == RVA
    if len(data) < pe.OPTIONAL_HEADER.SizeOfImage:  # a file on disk, not a dump
        data = pe.get_memory_mapped_image()
    text = next(s for s in pe.sections if s.Name.startswith(b".text"))
    t0, t1 = text.VirtualAddress, text.VirtualAddress + text.Misc_VirtualSize

    pdata = next(s for s in pe.sections if s.Name.startswith(b".pdata"))
    funcs = []
    for off in range(pdata.VirtualAddress, pdata.VirtualAddress + pdata.Misc_VirtualSize - 11, 12):
        b, e, _ = struct.unpack_from("<III", data, off)
        if b and e > b:
            funcs.append((b, e))
    funcs.sort()
    starts = [f[0] for f in funcs]

    def func_of(rva):
        i = bisect.bisect_right(starts, rva) - 1
        while i >= 0:
            b, e = funcs[i]
            if b <= rva < e:
                # chained unwind entries: walk back to the primary entry is not needed for naming
                return b
            if rva >= e and i < len(funcs) and b < rva - 0x100000:
                break
            i -= 1
        return None

    wanted = {}
    for t in targets:
        if t.startswith("0x"):
            wanted[int(t, 16) - base] = t
        else:
            off = data.find(t.encode() + b"\0")
            while off > 0 and data[off - 1] != 0:  # require a string start
                off = data.find(t.encode() + b"\0", off + 1)
            if off < 0:
                print(f"{t}: not found")
                continue
            wanted[off] = t

    # REX.W lea/mov/cmp with [rip+disp32] (modrm mod=00 rm=101), plus 32-bit lea without REX
    pat = re.compile(rb"[\x48\x4c](?:\x8d|\x8b|\x89|\x3b|\x39)[\x05\x0d\x15\x1d\x25\x2d\x35\x3d]", re.S)
    hits = {k: [] for k in wanted}
    for m in pat.finditer(data, t0, t1):
        i = m.start()
        disp = struct.unpack_from("<i", data, i + 3)[0]
        tgt = i + 7 + disp
        if tgt in wanted:
            hits[tgt].append(i)
    for tgt, name in wanted.items():
        print(f"== {name} @ {base + tgt:#x}")
        for i in hits[tgt]:
            f = func_of(i)
            print(f"   {base + i:#x} in func {base + f:#x}" if f is not None else f"   {base + i:#x} (no pdata)")


main()
