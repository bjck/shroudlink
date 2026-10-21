"""Assign every stackable item (plus gear and weapon gems) one chest category, based on its game category and debug name.

usage: python categorize.py <items.tsv from the chest_dump EML mod> <out dir, e.g. mod/server>

Writes categories.json (the editable default category file, by item debug name) and
items.tsv (item id <tab> debug name for every item, used to resolve names at runtime).
Rules are checked in order; the first match wins.
"""
import json
import re
import sys
from collections import defaultdict

# (category, game category or None for any, debug-name regex)
RULES = [
    ("Misc", None, r"^Key_"),
    ("Gems", "WeaponGem", r""),
    ("Spells", "Ammunition", r"_Spell_"),
    ("Ammo", "Ammunition", r""),
    ("Ammo", "Weapons", r"Grenade|Bomb|Throwable"),
    ("Ammo", "Consumables", r"Grenade|Bomb"),
    # gear (unstackable: moved whole, durability and stats included)
    ("Weapons", "Weapons", r"^(Craft_)?Weapon_"),
    ("Tools", "Weapons", r"^Tool_Torch"),
    ("Armor", "Equipment", r"^(?!Icon_|TestItem|Armor_Tint_TEST)"),
    ("Tools", "Tools", r""),
    ("Tools", "FishingRod", r""),
    ("Tools", "Instrument", r""),
    ("Dyes", "ColorPalette", r""),
    ("Collectibles", "Collectibles", r""),
    ("Food", "AnimalFood", r""),
    ("Potions", "Consumables", r"Consumable|Potion|Scroll|Bandage|RepairKit"),
    ("Food", "Consumables", r""),
    # materials
    ("Food", "Materials", r"raw_|Flour|Salt|Wheat|Spice|Saffron|CoffeeBeans|Pastry|SlurpBroth|MushroomMeat|Rice"),
    ("Terrain", "Materials", r"^Build_TerrainMaterial|FogTerrain|GlowDirt"),
    ("Ores", "Materials", r"Ore|_Bars?$|Metal_|Nails|Iron_Dust|Gold_Dust|Silver_Dust|(^|_)Coal|Sulfur|Nitrate|Black_Powder|Celesthium"),
    ("Gems", "Materials", r"Amber|Amethyst|Aquamarine|Diamond|Lapislazuli|Moonstone|Obsidian|Pearl|KryptCrystal|Marble|Archaic"),
    ("Wood", "Materials", r"Wood|Twigs|Resin|Tar$|Charcoal"),
    ("Stone", "Materials", r"Stone|Clay|Brick|Glass|Basalt|Flint"),
    ("Shroud", "Materials", r"Fog|Shroud|Goo|Fognest|FellMist|Sludge|Vapor"),
    ("Textiles", "Materials", r"Fabric|Linen|Leather|String|Wool|Padding|Paper|Cloth|OldBooks|Wax"),
    ("Plants", "Materials", r"Plant|Flax|Aloe|Flower|Night_|Chamomile|Rooibos|Wolfsbane|Indigo|Reed|PalmLeaves|Straw|Daylily|LavaPlant|Lily|Gentian|GleamRoot|Mushroom"),
    ("Monster", "Materials", r"Bone|Fur|Hide|Scale|Teeth|Gland|Blood|Claw|Tusk|Eye|Feather|Fish|Critter|Poison|Spider|Lizard|Trophy|Chitin|Talon|Ribs|Shell|Bristles|Sting|Jelly|Heart|Vorgoth|Sack|Organ|Ammoniak"),
    ("Alchemy", "Materials", r"Alchemical|Infusion|Antiseptic|Dust|Essence|Mauveine|Vitriol|Ointment|Medicine|Spark|Acid|Goo|Gel"),
    ("Misc", "Materials", r""),
    # building
    ("Plants", "BuildTools", r"Seedling|Seed"),
    ("Blocks", "BuildTools", r"^Block_|BuildTool_Voxel"),
    ("Decor", "BuildTools", r""),
    ("Misc", None, r""),
]

# items that stay with the player whatever a chest is called
NEVER_SORT = r"^Lockpick_"

# game categories whose unstackable items are still sorted (moved whole, stats included)
UNSTACKABLE_CATEGORIES = {"WeaponGem", "Weapons", "Equipment", "Tools", "FishingRod", "Instrument"}

# chest-name words -> categories (lower case; the mod also accepts singular forms)
ALIASES = {
    "food": ["Food"],
    "potions": ["Potions"],
    "consumables": ["Food", "Potions"],
    "ammo": ["Ammo"],
    "spells": ["Spells"],
    "gear": ["Weapons", "Armor"],
    "equipment": ["Weapons", "Armor"],
    "weapons": ["Weapons"],
    "armor": ["Armor"],
    "armour": ["Armor"],
    "shields": ["Armor"],
    "rings": ["Armor"],
    "jewelry": ["Armor"],
    "tools": ["Tools"],
    "magic": ["Spells"],
    "charges": ["Spells"],
    "arrows": ["Ammo"],
    "bombs": ["Ammo"],
    "grenades": ["Ammo"],
    "ores": ["Ores"],
    "metals": ["Ores"],
    "bars": ["Ores"],
    "gems": ["Gems"],
    "wood": ["Wood"],
    "stone": ["Stone"],
    "terrain": ["Terrain"],
    "dirt": ["Terrain"],
    "blocks": ["Blocks"],
    "building": ["Blocks", "Terrain", "Stone"],
    "decor": ["Decor"],
    "decorations": ["Decor"],
    "furniture": ["Decor"],
    "plants": ["Plants"],
    "seeds": ["Plants"],
    "monster": ["Monster"],
    "drops": ["Monster"],
    "shroud": ["Shroud"],
    "textiles": ["Textiles"],
    "cloth": ["Textiles"],
    "alchemy": ["Alchemy"],
    "dyes": ["Dyes"],
    "colors": ["Dyes"],
    "collectibles": ["Collectibles"],
    "collectables": ["Collectibles"],
    "collection": ["Collectibles"],
    "misc": ["Misc"],
    "other": ["Misc"],
    "miscellaneous": ["Misc"],
    "keys": ["Misc"],
    "materials": ["Ores", "Gems", "Wood", "Stone", "Shroud", "Textiles", "Plants", "Monster", "Alchemy"],
}


README = [
    "Chest categories for the chestsort server mod. Edit freely; changes apply within a few seconds, no restart needed.",
    "Name a chest with one or more category words, e.g. 'Food, Wood' or 'Ores & Gems'. Case and plural 's' do not matter.",
    "words: extra chest-name words for a category (the category name itself always works).",
    "items: item debug names; '*' is a wildcard, e.g. 'Food_T1_*'. A leading '-' removes items, e.g. '-*_Seedling'.",
    "Delete this file to get the defaults back. Problems are reported in chestsort.log.",
]


def main():
    src, out_dir = sys.argv[1], sys.argv[2]
    categories = defaultdict(list)
    all_items = []
    with open(src, encoding="utf-8") as f:
        next(f)
        for line in f:
            item_id, max_stack, game_cat, _name, _tags, debug = line.rstrip("\n").split("\t")
            all_items.append((debug, int(item_id)))
            # unstackable items carry per-item state; weapon gems are the exception worth sorting
            if (int(max_stack) <= 1 and game_cat not in UNSTACKABLE_CATEGORIES) or "DEPRECATED" in debug.upper() or "UNUSED" in debug.upper():
                continue
            if re.search(NEVER_SORT, debug):
                continue
            for cat, gcat, pattern in RULES:
                if (gcat is None or gcat == game_cat) and re.search(pattern, debug):
                    # unstackable leftovers (fists, test items, icons) are never sorted
                    if int(max_stack) > 1 or cat != "Misc" or pattern:
                        categories[cat].append(debug)
                    break

    words = defaultdict(list)
    for word, cats in ALIASES.items():
        for cat in cats:
            if word != cat.lower():
                words[cat].append(word)

    # hand-formatted so that words stay on one line and every item gets its own line
    lines = ["{", ' "_readme": [']
    lines += [f"  {json.dumps(r)}," for r in README]
    lines[-1] = lines[-1].rstrip(",")
    lines += [" ],", ' "categories": {']
    cats = sorted(categories)
    for i, cat in enumerate(cats):
        items = sorted(set(categories[cat]), key=str.lower)
        lines.append(f"  {json.dumps(cat)}: {{")
        lines.append(f'   "words": {json.dumps(words[cat])},')
        lines.append('   "items": [')
        lines += [f"    {json.dumps(n)}," for n in items]
        lines[-1] = lines[-1].rstrip(",")
        lines.append("   ]")
        lines.append("  }" + ("," if i < len(cats) - 1 else ""))
    lines += [" }", "}", ""]
    with open(f"{out_dir}/categories.json", "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(lines))
    with open(f"{out_dir}/items.tsv", "w", encoding="utf-8", newline="\n") as f:
        f.writelines(f"{item_id}\t{debug}\n" for debug, item_id in sorted(all_items))
    for cat in cats:
        print(f"{cat:13} {len(set(categories[cat]))}")


main()
