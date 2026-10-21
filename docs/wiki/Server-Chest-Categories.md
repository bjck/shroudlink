# Chest categories

`chestsort_categories.json` next to `enshrouded_server.exe` decides which items a named
chest pulls in. It is created with the defaults on first start. Edits apply **within a
few seconds, without a restart**. Mistakes are reported in `shroudlink.log`, and the
previous categories stay active until the file is valid again.

## Format

```json
{
  "categories": {
    "Ores": {
      "words": ["metals", "bars", "materials"],
      "items": ["Material_T*_Ore_*", "Material_T*_Bar_*", "-Material_T1_Ore_Test"]
    }
  }
}
```

- **The category name** always works as a chest-name word (`Ores`, `Ore`).
- **`words`** are extra chest-name words. A word may belong to several categories:
  `materials` fills a chest with all of them.
- **`items`** are item debug names:
  - `*` is a wildcard.
  - A leading `-` removes items, so `-*_Seedling` takes the seedlings out again.
  - A plain number is an item id.
- Delete the file to get the defaults back.

## How a chest name is read

1. The name is split at everything that is not a letter or digit (`Food, Wood`,
   `Ores & Gems` and `Food+Wood` all work).
2. Each word is lower-cased and looked up. A word longer than 3 letters that ends in `s`
   also works without it.
3. Unknown words are ignored. The chest takes the items of every category it found.

Item names are **not** matched: a chest called "Iron Bar" takes nothing unless "iron" or
"bar" is a category word (`bar` is one, for Ores).

## Default categories

| Category | Extra words |
|---|---|
| Alchemy | materials |
| Ammo | arrows, bombs, grenades |
| Armor | gear, equipment, armour, shields, rings, jewelry |
| Blocks | building |
| Collectibles | collectables, collection |
| Decor | decorations, furniture |
| Dyes | colors |
| Food | consumables |
| Gems | materials |
| Misc | other, miscellaneous, keys |
| Monster | drops, materials |
| Ores | metals, bars, materials |
| Plants | seeds, materials |
| Potions | consumables |
| Shroud | materials |
| Spells | magic, charges |
| Stone | building, materials |
| Terrain | dirt, building |
| Textiles | cloth, materials |
| Tools | |
| Weapons | gear, equipment |
| Wood | materials |

The defaults are generated from the game's item data by `research/scripts/categorize.py`.
Players with the addon see the server's actual categories in the MODS tab.
