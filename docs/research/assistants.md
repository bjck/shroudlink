# Assistants and production stations (research)

Build 1273043 (the 2026-10-21 server update). Addresses are of that build; hook points
and helpers are found by signature or call position, not by address.

## Who makes what

`research/scripts/eml/recipe_dump` exports the `RecipeRegistryResource` (2536 recipes),
the `WorkshopRegistryResource` and the `NpcCollection`; `research/scripts/recipes.py`
turns that into `mod/mods/assistants/recipes.tsv`.

- Every survivor has a workshop (`NpcResource.workshopId`); its assistant
  (`isCraftingAssistant`, e.g. `NPC_Workshop_Farmer_Assistant_01`) has the **same**
  workshop as the survivor (`NPC_Workshop_Farmer01`). The NPC entity carries
  `ActiveNpcState.npcId` (+0x0, the `NpcResource.id`, the same in every session) and
  `ecs::Crafting.workshopId` (+0x10).
- Survivor workshops craft instantly (the player's crafting menu). Timed production
  belongs to **stations**: 29 workshops with their own item (`WorkshopResource.item`),
  e.g. `Craftingstation_Farmer_Kitchen`, `Prop_production_T2_Cooking_Station`,
  `Prop_Production_T2_Smelter`. A station is unlocked by the survivor whose workshop has
  a recipe that makes its item: the Farmer makes the Kitchen, Oven, Cooking Station,
  Grill, Beehive, Butter Churn, Seedbed and the coffee makers; the Blacksmith the Forge,
  Smelter, Charcoal Kiln and Blast Furnace; and so on (every station has one).
- Food is station work: `Food_T5_Yakstew` is a Kitchen recipe (4 inputs, 5 stew, 360 s,
  a crafting prop in `requiredPropsMask`); `Food_T1_processed_grain_CorncobSkewer` a
  Cooking Station recipe (60 s, `requiredEnergy` 1500, i.e. fuel).
- Only the magic chests (`Prop_Decoration_T*_Storage_*_Magic`) and a few harvest refills
  have `InventoryCraftingStock` (crafting may pull from them).

## How a station runs (`factory_station` system, `FUN_1400f1610`)

A state machine over `ecs::FactoryStation` (104 bytes):

| Offset | Field |
|---|---|
| +0x0 | `state`: NoRecipe 0, MissingMaterials 1, Paused 2, Stopped 3, Running 4, MissingOutputSpace 5 |
| +0x4 | `runningRecipe` |
| +0x28 | `nextRecipe` |
| +0x2c | `nextRecipeInputCategorySelections` (4 × { id, selectedIndex }, 8 bytes each) |
| +0x50 | `recipeStart` (`Time`) |
| +0x60 | `useAllAvailableCategoryIngredients` |
| +0x61 | `isStoppedByPlayer` |
| +0x62 | `missingRequirements`: Recipe 1, EnergySourceInRange 2, EnergyAmount 4, InputMaterials 8, OutputSpace 0x10, AboveWater 0x20 |

- NoRecipe / MissingMaterials with `nextRecipe` set: `FUN_1400e9d40` → `FUN_1400e0430`
  checks and **consumes the inputs** (craft request with flag 8: no outputs), then
  `state = Running`, `runningRecipe = nextRecipe`, the selections are copied,
  `recipeStart = now`. Missing inputs: MissingMaterials. Missing energy: a dry run only
  (flag 0x40) and Paused.
- Running: when `craftingDuration × factoryProductionSpeedFactor` has passed,
  `FUN_1400e9900` crafts the **outputs** (flag 0x10: no inputs) and resets the state to
  NoRecipe; no room gives MissingOutputSpace. The next frame starts again with the same
  `nextRecipe`, so a station repeats a recipe for as long as it has material.
- Stopped with `isStoppedByPlayer == 0` starts again. Clearing `nextRecipe` while Running
  goes to `FUN_1400e9100` (stops the run; it calls `FUN_1401954b0` with the recipe, most
  likely a refund; not verified).
- The craft request (`FUN_1400e1d90`) names the **station itself** as the operator: the
  inputs come from the station's own inventory (16 slots) and the results go into it.
  The craft routine is `FUN_14020b3b0` → `FUN_140194a30(ctx, RecipeInfo, …, propsInRange,
  …, operator, amount)`: props check (`RecipeInfo +0x48`), inputs (`FUN_1401944e0`, or
  `FUN_140195210` for a check), water, then every output through
  `FUN_1401ac790(…, operator, item, amount × count)`. On success `CraftRecipeSuccessEvent`
  { workshopId, recipeId } is posted on the operator.

So an assistant does not need to create items: filling the station, setting `nextRecipe`
and emptying the station is enough, and the game keeps its own rules (time, fuel, power,
props, the server's production speed).

## Fuel is water (`fuel_dump`, `recipe_dump`)

- `ecs::Fuel` (16 bytes) holds two attributes of the entity, `FuelAmount` (0x05b0bf75) and
  `FuelMax` (0xa00e2601): +0x4 storage offset (u16), +0x6 flags (bit 0: set), the values as
  sint32 at the storage offset. Every frame `fuel_network` (`FUN_1400536f0`) copies them
  into `NetworkFuel` { fuel, fuelMax } with the change notice, so writing the attribute is
  enough. Which value is which is looked up once for all tanks.
- What fills it is liquid: `actor::TransferFuelEvent` (`PideToProp` / `PropToPide`: the
  Watering Can `Pide_Fuel_Watering_Can`, 100 charges, or the water pouch), and
  `fuel_setup` (`FUN_1400f3070`): automatic and rain refill. Wells
  (`Prop_NPC_Water_Well_*`: 250 charges, auto refill in 15 min, rain) and water barrels
  have `FuelCraftingStock`.
- Stations with a tank: Oven, Beehive, Laboratory, coffee brewer, Seedbed (150) and the
  Cooking Station (`T2_Fireplace_Factory`, 100). 87 station recipes take
  `RecipeInfo.waterCharges` per run (Kitchen 5, Seedbed 1–2, Laboratory up to 10 …); the
  craft routine takes them from fuel stock in range first (`ctx+0xb1`, `FUN_1402036f0`),
  then from the operator's tank (`FUN_1401ab220`).
- Heat is not consumed: hearths, campfires and the Cooking Station have `EnergyProvider`
  (1000 per second); `requiredEnergy` (1500–5000 on cooking recipes) is about heat sources
  near the station. The state machine's energy bits are `DynamicRotationalEnergyConsumer`
  (+4 sources nearby, +5 enough): water wheels and windmills.
- Burnable items (`ItemInfo.fuelSetup` +0x6e8, `ItemFuelData`): only Wood Logs and
  Hardwood (900 s) and Twigs (300 s), 1000 energy. `actor::RefillBurner` moves one of them
  from the player's hand into slot 0 of the interaction target (mode "one" transfer).

## Crops, trees and fishing (`farm_dump`)

- 66 plant templates have `ecs::Growth` (minimum duration, allowed soils with a speed
  factor) with `Level` and `Upgrade`: each level names an `upgradeEntity`, the stage the
  plant shows (`T1_Plant_Growth_Corn_Seedling_0`, `…_1`, `T1_Plant_Growth_Corn_2_harvest`).
- Crops' last stage holds the harvest in its inventory (`DefaultInventory`) and has
  `DestroyOnLoot` (+0x40 isActive, +0x41 shouldDestroy): looted empty, it goes. Trees and
  bushes end in a loot plant with `InteractionLootRefill` (e.g. date palm: 4 more every
  120 s, up to 12) and `InventoryCraftingStock`; tame animals refill the same way.
- Fishing is a client minigame. `ecs::FishingSpot` { spawnTable (Guid), radius } entities
  name a table, but most templates are marked UNUSED; players fish anywhere from
  `ecs::Fishing.defaultSpawnTable` (7ac6cdd5-…). `FishSpawnTableResource.fishEntries`
  { weight, timeFrame, item }; baits have `ItemInfo.baitItemSetup.fishBooster`
  { item, weightFactor } (ItemInfo +0x7f8).

## How plants grow, and replanting without the building system

- The plant **root** (the entity a seedling places; `Growth`, `Level`, `Upgrade`) stays for
  the plant's whole life. The `growth` system (`FUN_1400f3b20`) raises its level (attribute
  0x724bf568 in `ecs::Level`) when `level < max` (the last `Upgrade` level, capped by
  `customGrowthLimit`), the soil allows it (`FUN_1400e08a0`) and `DynamicGrowth.nextLevelTime`
  is due; with `nextLevelTime == 0` it sets level 1. It then schedules the next stage from the
  current time (`minDuration` × random scale × soil factor × level factor).
- The `upgrade` system (`FUN_140090df0`) looks up the `Upgrade` entry of the current level and,
  when the shown stage (`DynamicUpgrade { entityId, spawnedLevel }`) is not that one,
  destroys it (`FUN_1405f5a00`) and spawns the entry's `upgradeEntity`. It follows the level
  in both directions.
- The growth system also destroys a root whose stage entity is gone: a looted crop takes its
  root along, which is why players replant.
- `DynamicGrowth`, `DynamicUpgrade` and `DynamicInteractionLootRefill` are not ECS
  components (no `Component` base, not in the descriptor table): mods cannot reach them.
- **Replanting**: empty the ripe stage, set the root's level to 0. Its `nextLevelTime` lies in
  the past, so the next frame the growth system sets level 1 and the upgrade system swaps the
  empty stage for the first seedling stage, as it swaps stages while growing. The seedling
  item to charge is found from the stage's harvest (`DefaultInventory.defaultInventory`,
  unique per crop; `plants.tsv`, 27 crops). Seedling items place their root template (the
  root's Guid appears in the seedling's `ItemInfo`).
- Not verified in game: whether emptying the ripe stage alone makes the game remove it (if a
  system destroys emptied `DestroyOnLoot` stages on its own, the root dies before the reset).

## Making and taking items (core v7)

- `FUN_1401ac790(out, ctx, entity, slot, item, count, level?, rarity?, data?)` puts items
  into an inventory: the craft routine's output path (`FUN_140194a30`, slot 0 = any, the
  optionals `{ value, isSet }` unset). `out[0]` error, `out+8` what did not fit.
- `FUN_1401b3c00(out, ctx, slot, count, clear)` takes items out of a slot (the slot
  transfer's source side); with `HasPIDE` it also handles the item's entity.
- Both log the change in the context like the game's own transfers. The self-test creates
  5 plant fibre in a chest and removes 2.

## Inventory functions found on the way

- The slot transfer (`FUN_1401aed90`, the 15th call in quick stack) takes a mode: 0 whole
  stack, 1 half, 2 one item; 1 and 2 also split into an empty slot. `World::move_items`
  uses mode 2.
- The component lookup the core uses for `InventorySetup` writes the component's address
  **and size** (16 bytes) into its out parameter. The core passed 8 bytes, which once
  overwrote a saved register (the self-test crashed with `rbx = 0x54`, the size of
  `InventorySetup`); fixed in `game.rs`.
- Old build (for reference): `FUN_1401b0a50(out, ctx, slot, ItemInfo*, pide, count)` puts
  items into a slot, `FUN_1401b2d50(out, ctx, slot, count, 1)` removes them.

## The survivors' AI (`active_npc`)

From the reflection data (`server/.cache/types.json`) and the live component bytes:

- The in-memory `ActiveNpcState` is the live `DynamicActiveNpcState`: +0x0 `state`
  (`active_npc::State`: Idle 3, DecideTask 4, FindTarget 5, GoToTarget 6, Roam 7, Sleep 8,
  Sitting 9, Interacting 10, Eating 12, WaitingCloseToPlayer 13, FinishTask 17), +0x1
  `previousState`, +0x2 `decision` (`active_npc::Decision`: WantsToRoam 1, WantsToSitDown 2,
  WantsToInteract 3, WantsToSocialize 4, WantsToSleep 5, WantsToEat 6, WantsToIdle 7).
- Stations carry `ActiveNpcTarget` (`npcType` at +6: which `active_npc::NpcType` may use
  it, e.g. Blacksmith 4, Farmer 8; interaction keys mapping to `NpcInteractionSequence`s);
  `DynamicActiveNpcTarget` holds `isAvailable` (+0) and `usedByEntityId` (+0x18).
- Decision 3 and state 5 written into an idle NPC: the AI finds one of its stations, walks
  there (6), interacts (10) for a few seconds and finishes (17); seen with the Blacksmith at
  his forge. The state names in the binary are only the reflection table's; the AI code
  was not located.

## Playing a sequence on an NPC (fishing animation, not done)

- `ActorInput` (+0x8 `triggerContext`, +0x178 `triggerCallCounter`) stays all zeros on
  NPCs while they talk or interact: the NPC AI does not start its sequences through it.
- `NetworkActor` +0x0 `sequenceRuntimeId` (sequence resource id u64, trigger time u64,
  actor entity id u32, call index) holds the running sequence that clients play, e.g.
  `0x1599d5a4fb2` while the Blacksmith talks; empty while walking or standing.
- A fishing animation would need the player's fishing sequence id (recorded while a player
  fishes) written there with a fresh trigger time, a client that plays a player sequence on
  an NPC's animation graph, and a server that does not overwrite it the next frame. Not
  tried.

## Open

- Writing `FactoryStation` from outside the system skips the game's change notification
  (`FUN_140667ec0` before each write); in game the station's window showed the recipe all
  the same.
- Survivors and assistants only appear in the loaded world (near a player), so the self-test
  sees none; the station control is checked in game.
- Choosing the station an NPC interacts with (perhaps through `isAvailable` of the others).
