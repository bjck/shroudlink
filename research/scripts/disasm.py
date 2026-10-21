"""Disassemble a function (bounds from .pdata) or a byte range of a dump/exe.

usage: python disasm.py <exe> <va> [length]
"""
import struct
import sys

import capstone
import pefile


def main():
    path, va = sys.argv[1], int(sys.argv[2], 16)
    pe = pefile.PE(path, fast_load=True)
    base = pe.OPTIONAL_HEADER.ImageBase
    image = pe.get_memory_mapped_image()
    rva = va - base
    if len(sys.argv) > 3:
        end = rva + int(sys.argv[3], 16)
    else:
        pd = next(s for s in pe.sections if s.Name.startswith(b".pdata"))
        end = None
        for o in range(pd.VirtualAddress, pd.VirtualAddress + pd.Misc_VirtualSize - 11, 12):
            b, e, _ = struct.unpack_from("<III", image, o)
            if b <= rva < e:
                rva, end = b, e
                break
        if end is None:
            end = rva + 0x200
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    for insn in md.disasm(image[rva:end], base + rva):
        print(f"{insn.address:#x}: {insn.mnemonic:7} {insn.op_str}")


main()
