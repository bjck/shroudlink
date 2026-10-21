# Features (players)

## Chest sorting by name (everyone, no addon needed)

1. Place a chest and press **Rename**.
2. Give it one or more category words, e.g. `Food`, `Ores & Gems` or `Weapons, Armor`.
3. At the chest, **hold R** (quick stack), or use a Quick Stack Station nearby.

Every item of those categories moves from your backpack into the chest, even items the
chest does not hold yet. Then the game's normal quick stack runs as well.

- Case does not matter. Plural `s` is optional: `Ore` works like `Ores`.
- Unknown words are ignored, so `Bob's Food Box` means **Food**.
- Your server's category list is in the MODS tab (below) and on
  [[Chest categories|Server-Chest-Categories]].

**Equipped items are never deposited:** armour, rings, weapons, ammo, spell, glider,
backpack, the item in your hand and the action bar.

## MODS tab (addon)

![MODS tab](https://raw.githubusercontent.com/bjck/shroudlink/main/docs/screenshots/mods-tab.jpg)

The addon adds **MODS** to the player menu, next to SKILLS. On the left are the server's
mods, on the right their windows. chestsort's **Chest categories** window:

- lists every category with its number of items; click one to see its items;
- has a search field: type an item name to see which category, and so which chest
  name, it belongs to.

![Item search](https://raw.githubusercontent.com/bjck/shroudlink/main/docs/screenshots/item-search.jpg)

## Item search in chests (addon)

![Chest search](https://raw.githubusercontent.com/bjck/shroudlink/main/docs/screenshots/chest-search.jpg)

1. Open any chest.
2. Click **SEARCH ( / )** next to the BACKPACK title, or press `/`.
3. Type.

Items that do not match fade out, in your backpack and in the chest, live as you type.
The search stays active from chest to chest.

| Key | Effect |
|---|---|
| `/` | open the search |
| `Esc` | clear the search; press again to leave the field |
| `X` | clear and close the search |

## Slot locks (addon)

![Locked slots](https://raw.githubusercontent.com/bjck/shroudlink/main/docs/screenshots/item-locks.jpg)

Hover a slot of your backpack and press **L**, in a chest screen or in your inventory.
The slot gets a **gold rim**. Locked slots are never deposited: not by tapping R (move
all), not by holding R (quick stack), and not by a Quick Stack Station. Press **L**
again to unlock.

- The lock belongs to the **slot**, not the item. Move the item elsewhere and the lock
  stays on the old slot.
- Locks are kept on your PC per server, and they survive rejoins and server restarts.
- After joining, **open any chest once**. That is how the addon learns your backpack
  layout; until then the inventory view shows no rims.
- You can still move a locked item by hand.

## Travel to your map marker (addon)

Set a waypoint on the map (M, then right-click a spot), open **MODS → Travel** and click
**Travel to the marker**. You land there right away, from anywhere, the shroud included.
The server has to know which player you are first: open any chest once after joining.

## Lantern (addon)

![Lantern off and on](https://raw.githubusercontent.com/bjck/shroudlink/main/docs/screenshots/lantern.jpg)

**MODS → Lantern → Light on** puts a light around your character, with both hands free
for weapons and tools. It is the same light as the game's Ring of Light, without wearing
the ring. **Light off** switches it off again; leaving the server switches it off too.

## Storage: all chests as one box (addon)

**MODS → Storage** shows everything stored in the chests within 40 m of you as one grid,
largest stacks first. Click an item to take all of it from every chest into your
backpack. If the backpack is full, you get as much as fits.
