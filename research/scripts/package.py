"""Builds the release zips from a release build, the way the GitHub workflow does.

    cd mod && cargo build --release && cd ..
    python research/scripts/package.py [--out dist/build]

Writes, with SHA256SUMS.txt:
  shroudlink-core.zip                  the framework core (server), core/steam_api64.dll
  shroudlink-client.zip                the player addon
  shroudlink-mod-<name>.zip            one per mod in SERVER_MODS, shroudlink_mods/<name>.dll
  shroudlink-bigger-stacks-chests.zip  the EML data patches with UPDATE-SERVER / REPATCH
  chestsort-enshrouded.zip             everything for a server in one zip
The split zips unpack into one folder side by side (UPDATE-SERVER finds the core in core/
or repatch/). Their READMEs are in dist/packages/. The other mods in the workspace
(lantern, travel, hello_mod) are built but not released.
"""
import argparse
import hashlib
import pathlib
import re
import sys
import zipfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
MOD = ROOT / "mod"
DIST = ROOT / "dist"

SERVER_MODS = ["chestsort", "storage", "meter", "assistants", "gemforge"]


def version():
    text = (MOD / "Cargo.toml").read_text(encoding="utf-8")
    m = re.search(r'\[workspace\.package\][^\[]*?version\s*=\s*"([^"]+)"', text)
    if not m:
        sys.exit("no version in mod/Cargo.toml [workspace.package]")
    return m.group(1)


def tree(src, dest):
    """(file, path in the zip) for every file below src."""
    return [(f, f"{dest}/{f.relative_to(src).as_posix()}") for f in sorted(src.rglob("*")) if f.is_file()]


def server_files(target):
    pkg = DIST / "chestsort-enshrouded"
    files = [
        (pkg / "repatch" / "REPATCH.bat", "repatch/REPATCH.bat"),
        (pkg / "repatch" / "repatch.ps1", "repatch/repatch.ps1"),
        (target / "shroudlink_server.dll", "repatch/steam_api64.dll"),
    ]
    # the EML data mods; dist/.../repatch/mods is a copy of mod/eml for local updates
    files += tree(MOD / "eml", "repatch/mods")
    files += [(target / f"{m}.dll", f"shroudlink_mods/{m}.dll") for m in SERVER_MODS]
    files += [
        # the same file the DLLs embed as their default
        (MOD / "categories" / "categories.json", "chestsort_categories.json"),
        (pkg / "README.txt", "README.txt"),
        (pkg / "UPDATE-SERVER.bat", "UPDATE-SERVER.bat"),
        (pkg / "UPDATE-SERVER.ps1", "UPDATE-SERVER.ps1"),
    ]
    return files


def client_files(target):
    return [
        (target / "shroudlink_client.dll", "dinput8.dll"),
        (DIST / "shroudlink-client" / "README.txt", "README.txt"),
    ]


PACKAGES = DIST / "packages"


def core_files(target):
    # a subfolder, so unpacking into the server folder cannot replace Steam's DLL by accident
    return [
        (target / "shroudlink_server.dll", "core/steam_api64.dll"),
        (PACKAGES / "core" / "README.txt", "README-shroudlink-core.txt"),
    ]


def mod_files(target, name):
    # no chestsort_categories.json: the mods create it, and an update must keep the owner's
    return [
        (target / f"{name}.dll", f"shroudlink_mods/{name}.dll"),
        (PACKAGES / name / "README.txt", f"README-{name}.txt"),
    ]


def stacks_files():
    pkg = DIST / "chestsort-enshrouded"
    files = [
        (pkg / "repatch" / "REPATCH.bat", "repatch/REPATCH.bat"),
        (pkg / "repatch" / "repatch.ps1", "repatch/repatch.ps1"),
    ]
    files += tree(MOD / "eml", "repatch/mods")
    files += [
        (pkg / "UPDATE-SERVER.bat", "UPDATE-SERVER.bat"),
        (pkg / "UPDATE-SERVER.ps1", "UPDATE-SERVER.ps1"),
        (PACKAGES / "bigger-stacks-chests" / "README.txt", "README-bigger-stacks-chests.txt"),
    ]
    return files


def write_zip(path, files):
    missing = [str(f) for f, _ in files if not f.is_file()]
    if missing:
        sys.exit("missing (cargo build --release first?):\n  " + "\n  ".join(missing))
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        for f, name in files:
            z.write(f, name)
    print(f"{path.name}: {len(files)} files")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--target", type=pathlib.Path, default=MOD / "target" / "release")
    ap.add_argument("--out", type=pathlib.Path, default=DIST / "build")
    ap.add_argument("--tag", help="fail unless this tag (v1.2.3) matches the workspace version")
    a = ap.parse_args()

    v = version()
    if a.tag and a.tag != f"v{v}":
        sys.exit(f"tag {a.tag} does not match the workspace version {v} (mod/Cargo.toml)")

    a.out.mkdir(parents=True, exist_ok=True)
    contents = {
        "chestsort-enshrouded.zip": server_files(a.target),
        "shroudlink-client.zip": client_files(a.target),
        "shroudlink-core.zip": core_files(a.target),
        "shroudlink-bigger-stacks-chests.zip": stacks_files(),
    }
    contents.update({f"shroudlink-mod-{m}.zip": mod_files(a.target, m) for m in SERVER_MODS})
    zips = []
    for name, files in contents.items():
        zips.append(a.out / name)
        write_zip(zips[-1], files)

    sums = "".join(f"{hashlib.sha256(z.read_bytes()).hexdigest()}  {z.name}\n" for z in zips)
    (a.out / "SHA256SUMS.txt").write_text(sums, encoding="ascii", newline="\n")
    print(f"version {v} -> {a.out}")
    print(sums, end="")


if __name__ == "__main__":
    main()
