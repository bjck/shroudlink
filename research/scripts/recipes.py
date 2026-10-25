"""Production data for the assistants mod: which recipes the base's production stations run,
which station belongs to which workshop, and which survivor (and assistant) unlocks it.

usage: python recipes.py <recipes.txt from the recipe_dump EML mod> <items.tsv from chest_dump> <out recipes.tsv>

Rows of the output (tab separated, ids are the game's HashKey32 values):
  npc       <npc id> <workshop id> <assistant 0/1> <debug name>
  station   <workshop id> <station item> <unlocking workshop or 0> <station debug name>
  recipe    <recipe id> <workshop id> <seconds> <energy> <water> <props mask> <inputs> <outputs> <debug name> <water charges>
  category  <category id> <item:scaling,...>
inputs: item:<id>:<count> or cat:<category id>:<count>, comma separated; outputs: <id>:<count>.
Water charges are taken per run from Wells in range or the station's own tank.
Only timed recipes of stations (factory recipes) are written: those are the ones a station
runs on its own, which an assistant can operate.
"""
import sys


def main():
    dump, items_path, out_path = sys.argv[1:4]
    names = {}
    for line in open(items_path, encoding="utf-8"):
        parts = line.rstrip("\n").split("\t")
        if parts[0].isdigit():
            names[int(parts[0])] = parts[-1]

    workshops, npcs, recipes, categories = {}, [], [], []
    for line in open(dump, encoding="utf-8"):
        p = line.rstrip("\n").split("\t")
        if p[0] == "WORKSHOP":
            workshops[int(p[2])] = int(p[4])
        elif p[0] == "NPC" and p[3] != "0":
            npcs.append((int(p[1]), int(p[3]), p[2] == "true", p[6]))
        elif p[0] == "RECIPE":
            recipes.append(p)
        elif p[0] == "CATEGORY":
            categories.append((p[1], p[2]))

    npc_workshops = {w for _, w, _, _ in npcs}
    # station item -> the NPC workshop whose recipe makes it
    made_at = {}
    for r in recipes:
        workshop = int(r[2])
        if workshop in npc_workshops:
            for out in r[15].split(","):
                if out:
                    made_at.setdefault(int(out.split(":")[0]), workshop)

    def timed(r):
        return int(r[3]) > 0 and int(r[2]) in workshops and int(r[2]) not in npc_workshops

    stations = sorted({int(r[2]) for r in recipes if timed(r)})
    rows = []
    for npc, workshop, assistant, name in sorted(npcs):
        rows.append(f"npc\t{npc}\t{workshop}\t{int(assistant)}\t{name}")
    for w in stations:
        item = workshops[w]
        rows.append(f"station\t{w}\t{item}\t{made_at.get(item, 0)}\t{names.get(item, '?')}")
    for r in recipes:
        if timed(r) and r[15]:
            seconds = int(r[3]) / 1e9
            charges = r[16] if len(r) > 16 else "0"
            rows.append(f"recipe\t{r[1]}\t{r[2]}\t{seconds:g}\t{r[6]}\t{r[7]}\t{r[8]}\t{r[14]}\t{r[15]}\t{r[13]}\t{charges}")
    used = {i.split(":")[1] for r in recipes if timed(r) for i in r[14].split(",") if i.startswith("cat:")}
    for cid, items in categories:
        if cid in used:
            rows.append(f"category\t{cid}\t{items}")
    with open(out_path, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(rows) + "\n")
    print(f"{len(npcs)} npcs, {len(stations)} stations, {sum(r.startswith('recipe') for r in rows)} recipes, {len(used)} categories")
    unlinked = [names.get(workshops[w], w) for w in stations if not made_at.get(workshops[w])]
    if unlinked:
        print("stations without a survivor recipe:", ", ".join(map(str, unlinked)))


if __name__ == "__main__":
    main()
