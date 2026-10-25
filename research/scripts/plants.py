"""Crops for the assistants mod's gardener: which seedling replants a ripe crop.

usage: python plants.py <farm.txt from the farm_dump EML mod> <out plants.tsv>

A ripe crop is the last growth stage of a plant root (`ecs::Upgrade.levels[].upgradeEntity`);
it is told apart at runtime by the harvest it holds (`ecs::DefaultInventory.defaultInventory`).
Rows: crop <harvest inventory guid> <seedling item id> <plant root> <stage count>
Only plants whose last stage holds a harvest (crops, not trees) and that a seedling places.
"""
import sys
from collections import defaultdict


def main():
    dump, out_path = sys.argv[1:3]
    stages = defaultdict(list)  # root guid -> [(level, stage name, inventory guid)]
    names, seed_of = {}, {}
    for line in open(dump, encoding="utf-8"):
        p = line.split()
        if line.startswith("ROOTSTAGE ") and len(p) >= 5:
            root, name, level, stage = p[1], p[2], int(p[3]), p[4]
            inventory = p[5] if len(p) > 5 else ""
            stages[root].append((level, stage, inventory))
            names[root] = name
        elif line.startswith("SEEDROOT ") and len(p) >= 4:
            seed_of.setdefault(p[3], p[1])
    rows = []
    for root, levels in sorted(stages.items(), key=lambda x: names[x[0]]):
        level, stage, inventory = max(levels)
        others = {inv for lv, _, inv in levels if lv != level}
        # the ripe stage has a harvest of its own, unlike the seedling stages
        if root in seed_of and inventory and inventory not in others and "harvest" in stage.lower():
            rows.append(f"crop\t{inventory}\t{seed_of[root]}\t{names[root]}\t{len(levels)}")
    with open(out_path, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(rows) + "\n")
    print(f"{len(rows)} crops")


if __name__ == "__main__":
    main()
