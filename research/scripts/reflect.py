"""Query reflection_data.json produced by `kfc-parser extract-types`.

usage: python -I reflect.py <reflection_data.json> <regex> [--fields]
"""
import json
import re
import sys


def type_name(types, idx):
    t = types[idx]
    if t["primitiveType"] in ("TYPEDEF", "ARRAY", "BLOB_ARRAY", "STATIC_ARRAY", "OPTIONAL", "REFERENCE", "OBJECT_REFERENCE", "BLOB_OPTIONAL", "VARIANT", "BLOB_VARIANT") and "innerType" in t:
        return f"{t['primitiveType'].lower()}<{type_name(types, t['innerType'])}>"
    return t["qualifiedName"]


def main():
    path, pattern = sys.argv[1], re.compile(sys.argv[2])
    show_fields = "--fields" in sys.argv
    types = json.load(open(path))["types"]
    for t in types:
        if not pattern.search(t["qualifiedName"]):
            continue
        print(f"{t['qualifiedName']}  [{t['primitiveType']}, size={t['size']}]")
        if show_fields:
            for f in (t.get("structFields") or {}).values():
                print(f"    +0x{f['dataOffset']:04x} {f['name']}: {type_name(types, f['type'])}")
            for e in t.get("enumFields") or {}:
                print(f"    enum {e}")


main()
