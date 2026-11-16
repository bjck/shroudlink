"""Check shroudlink's hook signatures against a game build before deploying anything.

usage: python checksigs.py <enshrouded_server.exe> [--client <memory dump of enshrouded.exe>]

Server hook points come from mod/server/src/hooks.rs (POINTS), client ones from the
SIG_* constants in mod/client/src. Each must match exactly once in .text; the output lists
which features would stay on. The client exe on disk is DRM-packed, so client signatures
need a memory dump (dumpmod.py). Exit code 1 if any server hook point fails.
"""
import re
import sys
from pathlib import Path

import pefile

ROOT = Path(__file__).resolve().parents[2]


def text_section(path):
    pe = pefile.PE(str(path), fast_load=True)
    text = next(s for s in pe.sections if s.Name.rstrip(b"\0") == b".text")
    return text.get_data()


def matches(code, signature):
    pattern = b"".join(b"." if t.startswith("?") else re.escape(bytes([int(t, 16)])) for t in signature.split())
    return len(re.findall(pattern, code, re.S))


def server_points():
    source = (ROOT / "mod/server/src/hooks.rs").read_text(encoding="utf-8")
    return re.findall(r'HookPoint \{ name: "(\w+)", feature: Feature::(\w+), signature: "([0-9A-F? ]+)" \}', source)


def client_points():
    points = []
    for path in sorted((ROOT / "mod/client/src").glob("*.rs")):
        for name, sig in re.findall(r'const (SIG_\w+): &str = "([0-9A-F? ]+)";', path.read_text(encoding="utf-8")):
            points.append((f"{path.stem}::{name}", sig))
    return points


def main():
    args = sys.argv[1:]
    if not args:
        raise SystemExit(__doc__)
    server_exe = Path(args[0])
    client_dump = Path(args[args.index("--client") + 1]) if "--client" in args else None

    code = text_section(server_exe)
    features = {}
    print(f"server: {server_exe}")
    for name, feature, sig in server_points():
        n = matches(code, sig)
        ok = n == 1
        features.setdefault(feature, True)
        features[feature] &= ok
        print(f"  {'ok  ' if ok else 'FAIL'} {name:<14} {feature:<10} {n} match(es)")
    print("  features: " + ", ".join(f"{f} {'on' if ok else 'OFF'}" for f, ok in features.items()))
    failed = not all(features.values())

    if client_dump:
        code = text_section(client_dump)
        print(f"client: {client_dump}")
        for name, sig in client_points():
            n = matches(code, sig)
            print(f"  {'ok  ' if n == 1 else 'FAIL'} {name:<32} {n} match(es)")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
