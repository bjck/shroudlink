"""Generate the shortest unique wildcarded byte signature for function entry points.

usage: python mksig.py <exe> <va> [va ...]
Relative branch/call targets and RIP-relative displacements are wildcarded.
"""
import sys

import capstone
import pefile


def load_text(path):
    pe = pefile.PE(path, fast_load=True)
    text = next(s for s in pe.sections if s.Name.rstrip(b"\0") == b".text")
    return pe.OPTIONAL_HEADER.ImageBase + text.VirtualAddress, text.get_data()


def pattern_for(md, code, base, va, max_len=96):
    pat = []
    for insn in md.disasm(code[va - base: va - base + max_len], va):
        raw = list(insn.bytes)
        mask = [True] * len(raw)
        # wildcard the trailing rel32/disp32 for rip-relative operands and near calls/jumps
        if insn.disp_size == 4 and ("rip" in insn.op_str or insn.group(capstone.CS_GRP_JUMP) or insn.group(capstone.CS_GRP_CALL)):
            off = insn.disp_offset
            mask[off:off + 4] = [False] * 4
        if (insn.group(capstone.CS_GRP_JUMP) or insn.group(capstone.CS_GRP_CALL)) and insn.imm_size:
            off = insn.imm_offset
            mask[off:off + insn.imm_size] = [False] * insn.imm_size
        pat.extend(b if m else None for b, m in zip(raw, mask))
        yield pat


def count(code, pat):
    first = next(i for i, b in enumerate(pat) if b is not None)
    hits, start = 0, 0
    needle = bytes([pat[first]])
    while True:
        i = code.find(needle, start)
        if i == -1:
            return hits
        s = i - first
        if s >= 0 and all(b is None or code[s + j] == b for j, b in enumerate(pat)):
            hits += 1
            if hits > 1:
                return hits
        start = i + 1


def main():
    base, code = load_text(sys.argv[1])
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    md.detail = True
    for arg in sys.argv[2:]:
        va = int(arg, 16)
        sig = None
        for pat in pattern_for(md, code, base, va):
            if len(pat) >= 12 and count(code, pat) == 1:
                sig = pat
                break
        text = " ".join("??" if b is None else f"{b:02X}" for b in sig) if sig else "NOT UNIQUE"
        print(f"{va:#x}: {text}")


main()
