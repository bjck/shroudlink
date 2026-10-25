"""Crops for the assistants mod's gardener: which seedling replants a ripe crop.

usage: python plants.py <farm.txt from the farm_dump EML mod> <out plants.tsv> [croploot.txt]

A ripe crop is the last growth stage of a plant root (`ecs::Upgrade.levels[].upgradeEntity`);
it is told apart at runtime by the harvest it holds (`ecs::DefaultInventory.defaultInventory`).
Rows: crop <harvest inventory guid> <seedling item id> <plant root> <stage count> [harvest]
Only plants whose last stage holds a harvest (crops, not trees) and that a seedling places.

A ripe stage's `DefaultInventory` is applied only when a player picks it (scatter loot like
beans, and wheat, corn …), so nothing is stored to take. With the `croploot_dump` export
(`croploot.txt`) every crop row gets the harvest the inventory gives:
item:count,item:count (stacks that always spawn, at their least count).
"""
import re
import sys
from collections import defaultdict


def loot_of(path):
    """Harvest inventory guid -> 'item:count,...' from the croploot_dump export."""
    loot = {}
    for line in open(path, encoding="utf-8"):
        if not line.startswith("INVENTORY "):
            continue
        guid = line.split()[1]
        stacks = re.findall(r"countMax=(\d+) countMin=(\d+) item=\{value=(\d+)\}.*?spawnProbability=([\d.]+)", line)
        # nested groups and selectors (random picks) are not reproduced: those crops stay manual
        if "isSelector=true" in line or "groups=[]" not in line:
            continue
        sure = [f"{item}:{low}" for _, low, item, chance in stacks if float(chance) >= 1.0]
        if sure:
            loot[guid] = ",".join(sure)
    return loot


def main():
    dump, out_path = sys.argv[1:3]
    loot = loot_of(sys.argv[3]) if len(sys.argv) > 3 else {}
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
        # the ripe stage has a harvest of its own, unlike the seedling stages; some ripe
        # stages are scatter loot (beans: Scatter_Loot_…_Bean): the gardener only takes
        # those standing on a plant root, so wild scatter loot is never mistaken for a crop
        if root in seed_of and inventory and inventory not in others:
            row = f"crop\t{inventory}\t{seed_of[root]}\t{names[root]}\t{len(levels)}"
            if inventory in loot:
                row += f"\t{loot[inventory]}"
            rows.append(row)
    # a ripe stage two plants share (cattails and papyrus) cannot tell which seedling it
    # needs: those stay for the players
    count = defaultdict(int)
    for row in rows:
        count[row.split("\t")[1]] += 1
    rows = [row for row in rows if count[row.split("\t")[1]] == 1]
    # the growing stages of those crops, by their own inventory (when no other plant has it),
    # so the gardener's window can tell what grows where:
    #   stage <stage inventory guid> <ripe inventory guid of its crop>
    ripe_of = {row.split("\t")[3]: row.split("\t")[1] for row in rows}
    used = defaultdict(set)
    for root, levels in stages.items():
        for _, _, inventory in levels:
            if inventory:
                used[inventory].add(root)
    for root, levels in sorted(stages.items(), key=lambda x: names[x[0]]):
        if names[root] not in ripe_of:
            continue
        top = max(levels)[0]
        for inventory in sorted({inv for lv, _, inv in levels if lv != top and inv and len(used[inv]) == 1}):
            rows.append(f"stage\t{inventory}\t{ripe_of[names[root]]}")
    with open(out_path, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(rows) + "\n")
    crops = [r for r in rows if r.startswith("crop")]
    print(f"{len(crops)} crops, {sum(r.count(chr(9)) > 4 for r in crops)} with a harvest made on picking, {len(rows) - len(crops)} growing stages")


if __name__ == "__main__":
    main()
