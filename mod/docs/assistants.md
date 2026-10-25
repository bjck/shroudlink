# Working assistants: production orders

Status: `mods/assistants` on SDK v7. Unit tests and the self-test pass, and it was
verified in game on a local server with a copy of a real base (Oct 2026): orders at the
Oven and the Blast Furnace, water from Wells, a chain, the gardener, the fisher, walking
and working at stations, notifications. Research: `docs/research/assistants.md`.

## What it does

A player stands near a survivor or an assistant at their base, opens **MODS →
Assistants**, picks the survivor, clicks a product and orders an amount ("Make 5", or a typed number
and Order).
The survivor runs the matching station near it: it takes the ingredients from the
chests around it, the station makes the item in its own time, and the survivor puts the
results into the chests. Everyone connected gets a notification when an order is done.

Nothing is created from nothing. The game's own station does the crafting with its
crafting time, fuel, power, crafting props and the server's production speed, exactly
as for a player; the assistant only does the carrying and picks the recipe.

## Why stations, not the survivor's crafting menu

The research (`docs/research/assistants.md`) changed the original plan:

- Survivor workshops craft instantly; every timed recipe, food included, belongs to one
  of 29 **stations** (Kitchen, Cooking Station, Oven, Smelter, Loom …).
- Each station is unlocked by one survivor (the one whose workshop makes the station's
  item), and an assistant has the same workshop as its survivor. So "the Farmer's
  assistant makes the food" means: it runs the Farmer's stations.
- A station already repeats `nextRecipe` for as long as its own inventory holds the
  inputs, taking them at the start of a run and putting the results into its inventory
  at the end. Filling and emptying it is all an assistant has to do, so the mod needs no
  item creation (the riskiest primitive of the first plan).

## How a work step goes

Every 5 seconds, for every open order whose survivor is near a player (the base is only
loaded then):

1. Stations within 40 m of the survivor that belong to the recipe's workshop and are free
   (no recipe, or not running) or already on this recipe. If there are none, an **idle**
   station set to someone else's recipe is borrowed (no recipe, missing materials or
   stopped; never paused, which may be mid-run): its recipe and category options are kept
   with the order and put back when the order no longer needs the station. A station that
   runs something else is left alone ("the Blast Furnace is making Iron Bar"), and two
   orders never share one. The borrowed record keeps where the station stands, so after a
   restart (new entity ids) the order finds it again.
2. The results in those stations go into the chests within 40 m (chests that already hold
   the item first); results of the ordered item count towards the order.
3. Done when enough is in the chests: the stations get their recipe cleared (or the
   borrowed one back) and unused material goes back into the chests.
4. Otherwise: the runs still needed (minus runs going on and results not yet collected),
   shared among the stations, at most 5 runs of material per station at a time. The
   planner picks the option of a category input ("any meat") the chests have most of and
   tells the station which option it is; the station may switch to others
   (`useAllAvailableCategoryIngredients`).
5. A station with material gets `nextRecipe`; one without material and not running gets
   its recipe cleared, so an order that waits for ingredients does not block the station.
6. **Water**: if the recipe takes water charges and the station has a tank (Oven, Beehive,
   Laboratory, coffee brewer, Seedbed, Cooking Station), the survivor carries water from
   the Wells and water barrels within 40 m into the tank and fills it up, the way a player
   does with a Watering Can: the Well loses what the station gets, and Wells refill on
   their own. A station without a tank (the Kitchen) draws from Wells in its own range when
   it crafts, so it needs a Well next to it.

7. **Chains**: an order short of an ingredient that a station at this base makes orders
   it first, as a sub-order of the same player, at most 3 levels deep and never in a loop
   (`src/chain.rs`); the parent waits ("waiting for Sugar (being made)") and takes the
   ingredient from the chests once it is there. Cancelling a parent cancels its sub-orders,
   and a finished parent stops the ones still running.

Status shown in the window: working, needs <items>, waiting for <items> (being made),
needs water (place a Well), the station needs power, no room in the chests, <station> is
making <item>, no station for it nearby, waiting until someone is at the base, done.

The window lists open orders with their sub-orders, then the last finished ones; clicking
an open order cancels it, clicking a finished one clears it ("Clear finished" clears all).
Feedback on a click shows under the order row and as a notification.

### Walking and working at stations

While an order runs, the survivor's **home point** (`ActiveNpcStateSaved`: spawnPosition,
overrideRoamingRange, roamingRange) moves to its stations with a range of 3 m, so the
game's own AI walks them there and keeps them there; when the work stops, the home point
they had comes back (kept in the order file, so a restart cannot strand anyone). With an
assistant at the base the assistant walks and the survivor stays home. Within 8 m of the
site and idle, roaming or waiting near a player, they are sent to **work at a station**:
the live state (`DynamicActiveNpcState`) gets decision WantsToInteract (3) and state
FindTarget (5), and the game picks one of their stations (`ActiveNpcTarget.npcType`),
walks them there and plays its interaction sequence. An interaction lasts a few seconds;
they are sent again whenever they stand idle. Sleeping, eating and sitting NPCs are left
alone. Which of their stations they use is the game's choice.

"Chests" are the inventories within 40 m that are no station, NPC or animal. Survivors
and orders are matched by `NpcId`, so an order follows its survivor to another base and
survives restarts (`shroudlink_assistants.json`, written as `.new` and renamed).

## Standing jobs: the gardener and the fisher

Switched on per survivor in their window (a toggle), kept in the order file until switched
off, run while the base is loaded (`src/jobs.rs`). Harvests and catches are announced
together, at most one notification a minute; each survivor's window has a switch for the
notifications about their work (finished orders, sub-orders, harvests, catches).

- **Gardener** (the Farmer and the Farmer's assistant), every 10 seconds (plants that
  refill grow one item at a time): empties into the chests within 40 m
  what is ripe within 40 m: crops in their last growth stage (`ecs::DestroyOnLoot` with a
  growing plant, `ecs::Growth`, at the same spot), whose harvest sits in their inventory;
  afterwards the crop is marked looted (`DestroyOnLoot.shouldDestroy`), so the game removes
  it as after a player's harvest. And the fruit of planted trees and bushes and the produce
  of tame animals (`ecs::InteractionLootRefill`, planted ones are `InventoryCraftingStock`,
  animals `IsTame`), which refill on their own. **Replanting**: an emptied crop is started
  over with one matching seedling from the chests (`plants.tsv`): the plant root's level is
  set to 0, and the game's own growth and upgrade systems grow it again from the first stage,
  as they do while it grows. The seedling goes first and comes back if that fails; without a
  seedling the crop is removed as in vanilla. No new plants are placed: only where one grew.
- **Fisher** (the Fisher): one catch a minute, at most 3 made up after a pause. Each catch
  takes one bait from the chests, draws a fish from the spawn table of a fishing spot
  within 60 m (`ecs::FishingSpot`) or the table players fish from anywhere, weighted by what
  the bait attracts (`baitItemSetup.fishBooster`), and **creates** the fish in a chest
  through the game's own function (core v7, `server/src/items.rs`). The bait goes first,
  so a failure can cost a bait, never duplicate a fish; with no room the bait goes back.
  The fisher does not check for water nearby (no server query for it is known).

## Files

| Path | What |
|---|---|
| `mods/assistants/recipes.tsv` | generated by `research/scripts/recipes.py` from the `recipe_dump` EML export: survivors, stations, 333 station recipes, input categories |
| `src/recipes.rs` | the table: stations of a survivor, products, recipes for a product |
| `src/planner.rs` | pure: runs needed, what to move into a station for the next runs, missing items |
| `src/base.rs` | the world behind a trait (`WorldBase` from the SDK); `FactoryStation` layout |
| `src/work.rs` | the work step, release (done / cancel); a simulated station (`fake`) for the tests |
| `src/orders.rs` | orders, status, the order file |
| `src/names.rs` | readable names from debug names |
| `src/jobs.rs` | standing jobs: harvesting and replanting, fishing |
| `plants.tsv`, `src/plants.rs` | generated by `research/scripts/plants.py`: ripe crop (by its harvest) -> seedling |
| `fishing.tsv`, `src/fishing.rs` | generated by `research/scripts/fishing.py` from the `farm_dump` export: spawn tables, baits; the draw |
| `src/chain.rs` | pure: sub-orders for missing ingredients |
| `src/walk.rs` | home points: walking to the work and back |
| `src/lib.rs` | the mod: menu entry, windows, tick |

Core (HostApi v6): `World::entities_near(entity, radius, "Component")` and
`World::move_items(from, slot, to, count)`; v7 (feature `items`): `World::give_items` and
`World::remove_items` through the game's `FUN_1401ac790` / `FUN_1401b3c00`. All with
self-test cases.

## Performance

A work step runs on the game thread, so a slow one makes every player and NPC
rubberband. A step on a base with 150 chests takes a few milliseconds; the mod logs
`slow work step` above 50 ms. Two traps cost half a second each (fixed in the core):
looking up chest names (an unknown name key rescans the heap) and scanning every possible
entity id.

## Verified in game

Orders at the Oven (water from a Well) and the Blast Furnace (borrowed from the player's
Glass recipe and given back), a sub-order, the gardener (refilling palms, replanting a
bean), the fisher, walking to the work, the Blacksmith working at his forge, notifications.
Not verified: whether a stopped run refunds its inputs on cancel.

## Later

- Planting on empty soil (new plants): needs the server side of placing a seedling
  (building system).
- Embedded UI: a section in the survivor's own crafting screen (needs a client building
  block for the crafting panel).
- Working at the very station that makes the order (the game picks one of theirs), and a
  fishing animation for the fisher (findings in `docs/research/assistants.md`).
- Keeping seedlings stocked (Seedbed orders), and the crops that end in scatter loot
  (beans and ten more), which the gardener cannot harvest yet.
