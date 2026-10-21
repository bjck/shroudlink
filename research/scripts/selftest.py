"""Run the shroudlink self-test in a local dedicated server, no game client needed.

usage: python selftest.py [--server DIR] [--no-install] [--timeout SECONDS] [--break HOOK ...]

Backs up the server's savegame, installs the freshly built DLLs (core as steam_api64.dll,
mods into shroudlink_mods/), starts the server with the self-test marker, waits for
shroudlink_selftest.txt, stops the server and restores the savegame (the tests move items).
Exit code 0 when every test passed. `--break move_all` simulates a game update that broke
that hook point (names in mod/server/src/hooks.rs): only its feature may switch off.
"""
import argparse
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
RELEASE = ROOT / "mod" / "target" / "release"
MARKER = "shroudlink_selftest"
REPORT = "shroudlink_selftest.txt"


def stop_server():
    subprocess.run(["taskkill", "/IM", "enshrouded_server.exe", "/F"], capture_output=True)
    for _ in range(100):
        out = subprocess.run(["tasklist", "/FI", "IMAGENAME eq enshrouded_server.exe"], capture_output=True, text=True).stdout
        if "enshrouded_server.exe" not in out:
            return
        time.sleep(0.2)
    raise SystemExit("the server does not stop")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--server", type=Path, default=ROOT / "server")
    ap.add_argument("--no-install", action="store_true", help="test the DLLs already installed")
    ap.add_argument("--timeout", type=int, default=180)
    ap.add_argument("--break", dest="broken", action="append", default=[], help="hook point to treat as missing")
    args = ap.parse_args()
    server = args.server.resolve()
    save = server / "savegame"

    stop_server()
    backup = Path(tempfile.mkdtemp(prefix="shroudlink_selftest_")) / "savegame"
    shutil.copytree(save, backup)
    print(f"savegame backed up to {backup}")
    try:
        if not args.no_install:
            shutil.copy2(RELEASE / "shroudlink_server.dll", server / "steam_api64.dll")
            mods = server / "shroudlink_mods"
            mods.mkdir(exist_ok=True)
            for dll in ("chestsort.dll", "hello_mod.dll"):
                shutil.copy2(RELEASE / dll, mods / dll)
            print("installed the DLLs from mod/target/release")
        (server / REPORT).unlink(missing_ok=True)
        (server / MARKER).write_text("".join(f"break {name}\n" for name in args.broken))
        proc = subprocess.Popen([str(server / "enshrouded_server.exe")], cwd=server, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        start = time.time()
        report = server / REPORT
        while time.time() - start < args.timeout:
            if report.exists() and "RESULT" in report.read_text(errors="replace"):
                break
            if proc.poll() is not None:
                break
            time.sleep(1)
        crashed = proc.poll() is not None
        stop_server()
        text = report.read_text(errors="replace") if report.exists() else ""
        if crashed and "RESULT" not in text:
            text += f"FAIL server: exited with code {proc.returncode} during the test\nRESULT crash\n"
        elif "RESULT" not in text:
            text += f"FAIL timeout: no result within {args.timeout} s\nRESULT timeout\n"
    finally:
        stop_server()
        (server / MARKER).unlink(missing_ok=True)
        shutil.rmtree(save, ignore_errors=True)
        shutil.copytree(backup, save)
        shutil.rmtree(backup.parent, ignore_errors=True)
        print("savegame restored")
    print()
    print(text, end="")
    sys.exit(0 if "RESULT ok" in text else 1)


if __name__ == "__main__":
    main()
