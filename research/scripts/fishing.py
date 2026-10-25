"""Fishing data for the assistants mod's fisher: spawn tables, what each bait attracts,
and the table a player uses where no fishing spot says otherwise.

usage: python fishing.py <farm.txt from the farm_dump EML mod> <out fishing.tsv>

Rows of the output (tab separated):
  default  <spawn table guid>                      (ecs::Fishing.defaultSpawnTable of the player)
  table    <guid> <fish item>:<weight>,...          (FishSpawnTableResource.fishEntries)
  bait     <bait item> <fish item>:<factor>,...     (ItemInfo.baitItemSetup.fishBooster)
Shoes and trash are left out: the fisher only lands fish.
"""
import re
import sys

ENTRY = re.compile(r"item=\{value=(\d+)\} timeFrame=\w+ weight=([\d.e-]+)")
BOOST = re.compile(r"item=\{value=(\d+)\} weightFactor=([\d.e-]+)")


def main():
    dump, out_path = sys.argv[1:3]
    rows = []
    for line in open(dump, encoding="utf-8"):
        if line.startswith("SPOT ") and " keen::ecs::Fishing " in line:
            m = re.search(r"defaultSpawnTable=([0-9a-f-]{36})", line)
            if m and not any(r.startswith("default\t") for r in rows):
                rows.append(f"default\t{m.group(1)}")
        elif line.startswith("FISHTABLE "):
            guid = line.split()[1]
            fish = line.split("shoeEntries=")[0]
            entries = [f"{item}:{float(w):g}" for item, w in ENTRY.findall(fish) if float(w) > 0]
            if entries:
                rows.append(f"table\t{guid}\t{','.join(entries)}")
        elif line.startswith("BAITITEM "):
            item = line.split()[1]
            boosts = [f"{fish}:{float(f):g}" for fish, f in BOOST.findall(line)]
            rows.append(f"bait\t{item}\t{','.join(boosts)}")
    with open(out_path, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(rows) + "\n")
    print(f"{sum(r.startswith('table') for r in rows)} tables, {sum(r.startswith('bait') for r in rows)} baits, default: {any(r.startswith('default') for r in rows)}")


if __name__ == "__main__":
    main()
