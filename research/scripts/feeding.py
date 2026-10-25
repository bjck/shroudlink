"""Animal feeders for the assistants mod's farmer: which food a feeder takes, and the
Farmer's workshop recipe that makes it.

usage: python feeding.py <recipes.txt from the recipe_dump EML mod> <items.tsv> <out feeding.tsv>

A placed feeder is known at runtime by its building item (`ecs::UsedItem`), e.g.
Animal_Farm_Feeder_Goat; its food is the item named for the same animal
(Material_T1_Goat_Feeding), made in the Farmer's workshop (no station: crafted at once).
Rows, in the format of recipes.tsv where they match it:
  feeder <feeder item> <food item> <feeder debug name>
  recipe <id> <workshop> 0 0 0 0 <inputs> <outputs> <name> 0
  category <id> <item:scaling,...>   (the categories those recipes take)
"""
import re
import sys


def main():
    recipes_path, items_path, out_path = sys.argv[1:4]
    names = {}
    for line in open(items_path, encoding="utf-8"):
        p = line.rstrip("\n").split("\t")
        if len(p) >= 2 and p[0].isdigit():
            names[int(p[0])] = p[1]
    food = {}  # animal -> food item
    feeders = {}  # animal -> feeder item
    for item, name in names.items():
        m = re.match(r"^Material_T\d+_([A-Za-z]+)_Feeding$", name)
        if m:
            food[m.group(1).lower()] = item
        m = re.match(r"^Animal_(?:Farm|Pet)_Feeder_([A-Za-z]+)$", name)
        if m:
            feeders[m.group(1).lower()] = item
    foods = set(food.values())
    recipes, categories, used = [], {}, set()
    for line in open(recipes_path, encoding="utf-8"):
        p = line.rstrip("\n").split("\t")
        if p[0] == "CATEGORY" and len(p) >= 3:
            categories[p[1]] = p[2]
        elif p[0] == "RECIPE" and len(p) >= 16:
            outputs = p[15]
            if any(int(o.split(":")[0]) in foods for o in outputs.split(",") if ":" in o):
                recipes.append(f"recipe\t{p[1]}\t{p[2]}\t0\t0\t0\t0\t{p[14]}\t{outputs}\t{p[13]}\t0")
                used.update(i.split(":")[1] for i in p[14].split(",") if i.startswith("cat:"))
    rows = [f"feeder\t{feeders[a]}\t{food[a]}\t{names[feeders[a]]}" for a in sorted(feeders) if a in food]
    rows += recipes
    rows += [f"category\t{c}\t{categories[c]}" for c in sorted(used) if c in categories]
    with open(out_path, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(rows) + "\n")
    print(f"{sum(r.startswith('feeder') for r in rows)} feeders, {len(recipes)} recipes, {len(used)} categories")


if __name__ == "__main__":
    main()
