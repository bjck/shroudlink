//! What the assistants need from the world, behind a trait so the work loop can be tested
//! with a simulated base. [`WorldBase`] is the real one, built from the SDK.
//!
//! A production station (`ecs::FactoryStation`, 104 bytes) runs on its own: while
//! `nextRecipe` is set it starts the recipe whenever its own inventory holds the inputs
//! (consumed at the start), and puts the outputs into that inventory when the crafting
//! time is up (`FUN_1400e0430` start, `FUN_1400e9900` finish, through the game's craft
//! routine `FUN_140194a30` with the station as the operator). The assistant only fills
//! the station, picks the recipe and empties it.

use std::sync::atomic::{AtomicU8, Ordering};

use shroudlink_sdk::{Slot, World};

/// `ecs::FactoryStation`: +0x0 state, +0x4 runningRecipe, +0x28 nextRecipe,
/// +0x2c nextRecipeInputCategorySelections (4 x { id u32, selectedIndex u8 }),
/// +0x60 useAllAvailableCategoryIngredients, +0x61 isStoppedByPlayer,
/// +0x62 missingRequirements (Bitmask8<FactoryStationRequirement>).
mod factory {
    pub const STATE: usize = 0x0;
    pub const RUNNING: usize = 0x4;
    pub const NEXT: usize = 0x28;
    pub const NEXT_SELECTIONS: usize = 0x2c;
    pub const SELECTION_SIZE: usize = 8;
    pub const SELECTIONS: usize = 4;
    pub const USE_ALL_OPTIONS: usize = 0x60;
    pub const STOPPED_BY_PLAYER: usize = 0x61;
    pub const MISSING: usize = 0x62;
}

/// `FactoryStationState` (NoRecipe, MissingMaterials, Paused, Stopped, Running, MissingOutputSpace)
pub mod state {
    #[cfg(test)]
    pub const NO_RECIPE: u8 = 0;
    #[cfg(test)]
    pub const MISSING_MATERIALS: u8 = 1;
    /// in the middle of a run that waits: not to be taken over
    pub const PAUSED: u8 = 2;
    pub const RUNNING: u8 = 4;
}

/// `FactoryStationRequirement` bits of `missingRequirements`
pub mod missing {
    pub const ENERGY_SOURCE: u8 = 1 << 1;
    pub const ENERGY_AMOUNT: u8 = 1 << 2;
    pub const ABOVE_WATER: u8 = 1 << 5;
}

/// `ecs::Fuel` (16 bytes): the entity's `FuelAmount` / `FuelMax` attributes (hashes
/// 0x05b0bf75 / 0xa00e2601): +0x4 storage offset (u16), +0x6 flags (bit 0: set), the
/// values as sint32 at the storage offset in the attribute set's order. The game copies
/// them into `NetworkFuel` { fuel, fuelMax } every frame (`fuel_network`, `FUN_1400536f0`),
/// which also tells which value is which. "Fuel" is water: Wells (`FuelCraftingStock`)
/// hold it, the Watering Can carries it, rain refills it.
mod fuel {
    pub const STORAGE_OFFSET: usize = 0x4;
    pub const FLAGS: usize = 0x6;
}

/// Which of the two `Fuel` values is the amount (0 or 1; 255 until a tank that is not full
/// showed it). The game looks both up once for all tanks.
static AMOUNT_INDEX: AtomicU8 = AtomicU8::new(u8::MAX);

/// Water in a station's or a Well's tank.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tank {
    pub amount: u32,
    pub max: u32,
}

/// A survivor's home point: they wander within `range` metres of `pos`
/// (`ecs::ActiveNpcStateSaved`, 48 bytes: +0x0 spawnPosition float3 in metres,
/// +0xc overrideRoamingRange, +0x10 roamingRange; read at a real base, as laid out).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Home {
    pub pos: [f32; 3],
    pub override_range: bool,
    pub range: f32,
}

mod npc_saved {
    pub const SPAWN: usize = 0x0;
    pub const OVERRIDE_RANGE: usize = 0xc;
    pub const RANGE: usize = 0x10;
}

/// `active_npc::State`, the first byte of `ecs::ActiveNpcState` as it is in memory (not
/// the reflected layout): Idle 3, DecideTask 4, Roam 7, WaitingCloseToPlayer 13 (set in
/// game: an NPC stuck waiting near a player went off to its next task at once).
mod npc_state {
    pub const IDLE: u8 = 3;
    pub const DECIDE_TASK: u8 = 4;
    pub const FIND_TARGET: u8 = 5;
    pub const ROAM: u8 = 7;
    pub const WAITING_CLOSE_TO_PLAYER: u8 = 13;
}

/// `active_npc::Decision` (`DynamicActiveNpcState.decision`, +0x2 of the live state)
mod npc_decision {
    pub const WANTS_TO_INTERACT: u8 = 3;
}

/// Something a gardener can harvest.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Harvest {
    /// a grown crop (the last growth stage of a plant): its inventory holds the harvest,
    /// and the game removes it once it is looted (`ecs::DestroyOnLoot`)
    Crop,
    /// a player-planted tree or bush, or a tame animal: the inventory refills on its own
    /// (`ecs::InteractionLootRefill`)
    Refill,
}

/// `ecs::Level` (16 bytes): an attribute set like `ecs::Fuel` (+0x4 storage offset, +0x6
/// flags, two u32 values: the plant's growth level, attribute 0x724bf568, and its maximum);
/// `ecs::NetworkLevel.level` (u16) tells which value is the level. The `growth` system
/// (`FUN_1400f3b20`) raises the level of a plant root whose next stage is due
/// (`level < max`, `DynamicGrowth.nextLevelTime <= now`), and the `upgrade` system
/// (`FUN_140090df0`) replaces the shown stage (`DynamicUpgrade.entityId`) whenever it
/// does not match the level, downwards too. A root at level 0 with its time in the past
/// therefore grows again from stage 1.
mod level {
    pub const STORAGE_OFFSET: usize = 0x4;
    pub const FLAGS: usize = 0x6;
}

/// `ecs::DestroyOnLoot` (104 bytes): +0x40 isActive, +0x41 shouldDestroy.
/// `ecs::DefaultInventory`: defaultInventory (Guid) at +0x0, keepOnRandomLoot +0x10
mod default_inventory {
    pub const APPLIED: usize = 0x11;
}

mod destroy_on_loot {
    pub const IS_ACTIVE: usize = 0x40;
    pub const SHOULD_DESTROY: usize = 0x41;
}

#[derive(Clone, Debug, PartialEq)]
pub struct Station {
    pub entity: u32,
    /// `ecs::Crafting.workshopId` (+0x10)
    pub workshop: u32,
    pub state: u8,
    pub running: u32,
    pub next: u32,
    /// the category options chosen for `next`: (category, option index)
    pub selections: Vec<(u32, u8)>,
    pub missing: u8,
}

pub trait Base {
    /// Survivors and assistants near an entity: (entity, workshop). At runtime the NPC
    /// components hold no `NpcId` (in memory `ActiveNpcState` starts with live state, not
    /// the reflected `npcId`), so NPCs are known by their `ecs::Crafting.workshopId`
    /// (+0x10), which a survivor and their assistant share; villagers have none.
    fn npcs_near(&self, entity: u32, radius: f64) -> Vec<(u32, u32)>;
    fn stations_near(&self, entity: u32, radius: f64) -> Vec<Station>;
    /// Storage around an entity: inventories that are no station, NPC or animal.
    fn chests_near(&self, entity: u32, radius: f64) -> Vec<u32>;
    fn slots(&self, entity: u32) -> Vec<Slot>;
    /// Moves up to `count` items of a stack (`u32::MAX`: the whole stack, spread over
    /// partial stacks and free slots).
    fn move_items(&self, from: u32, slot: u64, to: u32, count: u32) -> u32;
    /// Sets the station's next recipe (0: none) with the category options to use.
    fn set_recipe(&self, station: u32, recipe: u32, selections: &[(u32, u8)]) -> bool;
    /// The water in an entity's tank; None without one.
    fn tank(&self, entity: u32) -> Option<Tank>;
    /// Sets the water in a tank (at most its maximum).
    fn set_tank(&self, entity: u32, amount: u32) -> bool;
    /// Wells (fuel stock: `FuelCraftingStock`) around an entity.
    fn wells_near(&self, entity: u32, radius: f64) -> Vec<u32>;
    /// Crops, player-planted trees and bushes, and tame animals around an entity.
    fn harvestables_near(&self, entity: u32, radius: f64) -> Vec<(u32, Harvest)>;
    /// Growing crops around an entity: plant roots (`ecs::Growth`) whose stage is no
    /// refilling plant (those are grown trees and bushes), as positions.
    fn growing_near(&self, entity: u32, radius: f64) -> Vec<[f64; 3]>;
    /// Tells the game an emptied crop was looted, so it goes as after a player's harvest.
    fn finish_crop(&self, crop: u32) -> bool;
    /// Creates plain items in an inventory (core v7); how many were created.
    fn give(&self, entity: u32, item: u32, count: u32) -> u32;
    /// Removes items from a slot (core v7); how many were removed.
    fn remove(&self, entity: u32, slot: u64, count: u32) -> u32;
    /// The seedling that replants a ripe crop, if it is one the table knows.
    fn crop_seedling(&self, crop: u32) -> Option<u32>;
    /// The harvest a ripe scatter-loot crop gives when picked, if it was not picked yet
    /// (`ecs::DefaultInventory.applied`, +0x11, still 0).
    fn crop_loot(&self, crop: u32) -> Option<Vec<(u32, u32)>>;
    /// Marks such a crop picked (`applied` = 1), before its harvest is made: a crop that
    /// stays (replanting failed) is never harvested twice.
    fn mark_looted(&self, crop: u32) -> bool;
    /// A scatter-loot crop picked already (marked, nothing in it): it waits to be replanted.
    fn crop_picked(&self, crop: u32) -> bool;
    /// The crop stages standing on plant roots around an entity, growing or ripe, of the
    /// kinds the crop table knows: (stage, seedling of its crop, ripe).
    fn crops_near(&self, entity: u32, radius: f64) -> Vec<(u32, u32, bool)>;
    /// Starts a ripe crop's plant over from its first growth stage (after its harvest was
    /// taken), as the game's growth does with each stage; false if that was not possible.
    fn replant(&self, crop: u32) -> bool;
    fn position(&self, entity: u32) -> Option<[f64; 3]>;
    /// An assistant rather than the survivor: assistants carry an inventory
    /// (`NPC_Workshop_*_Assistant_01` templates have `InventorySetup`, survivors none).
    fn is_assistant(&self, npc: u32) -> bool;
    /// A survivor's home point.
    fn npc_home(&self, npc: u32) -> Option<Home>;
    fn set_npc_home(&self, npc: u32, home: Home) -> bool;
    /// Makes an idle, roaming or waiting NPC decide on a new task, so a moved home point
    /// counts at once (as the game's own roaming input does); a sleeping, eating or
    /// sitting one is left alone.
    fn nudge_npc(&self, npc: u32) -> bool;
    /// Makes an idle, roaming or waiting NPC go and work at one of its stations, as its AI
    /// does now and then on its own: the decision to interact, then the search for a target
    /// (stations whose `ActiveNpcTarget.npcType` is theirs), the walk there and the
    /// station's interaction sequence. Which of their stations is the game's choice.
    fn work_at_station(&self, npc: u32) -> bool;
    /// Animal feeders of the base an entity stands in (inside its build zone), else within
    /// `radius` of it: (feeder, its building item, `ecs::UsedItem`).
    fn feeders_near(&self, entity: u32, radius: f64) -> Vec<(u32, u32)>;
    /// The spawn table of the nearest fishing spot (`ecs::FishingSpot.spawnTable`, a Guid
    /// in text form), if one is around.
    fn fishing_spot_near(&self, entity: u32, radius: f64) -> Option<String>;
}

pub struct WorldBase<'a> {
    pub world: World<'a>,
}

fn u32_at(data: &[u8], at: usize) -> u32 {
    data.get(at..at + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()))
}

/// Edge of a base's build zone (a cube around its flame altar) by the altar's level, in
/// metres (`BalancingTable.buildzoneSizesPerAltarLevel`, `research/scripts/eml/base_dump`).
const BUILD_ZONE: [f64; 6] = [0.0, 40.0, 80.0, 120.0, 160.0, 160.0];

impl WorldBase<'_> {
    /// The base an entity stands in: (flame altar, its position, half the zone's edge). An
    /// altar is the entity with `ecs::BaseIdComponent`; its level is `NetworkLevel` (+0x0, u16).
    fn base_zone(&self, entity: u32) -> Option<(u32, [f64; 3], f64)> {
        let at = self.world.position(entity)?;
        let farthest = BUILD_ZONE.iter().copied().fold(0.0, f64::max) / 2.0 * 1.75;
        self.world.entities_near(entity, farthest, "BaseIdComponent").into_iter().find_map(|altar| {
            let level = self.world.component(altar, "NetworkLevel").map_or(0, |l| u16::from_le_bytes([l[0], l[1]]) as usize);
            let half = BUILD_ZONE.get(level).or(BUILD_ZONE.last()).copied()? / 2.0;
            let center = self.world.position(altar)?;
            ((0..3).all(|i| (at[i] - center[i]).abs() <= half) && half > 0.0).then_some((altar, center, half))
        })
    }

    /// The `Fuel` bytes, where its two values sit, and which of them is the amount.
    fn fuel_layout(&self, entity: u32) -> Option<(Vec<u8>, [usize; 2], usize)> {
        let f = self.world.component(entity, "Fuel")?;
        let n = self.world.component(entity, "NetworkFuel")?;
        if f.get(fuel::FLAGS)? & 1 == 0 {
            return None;
        }
        let at = u16::from_le_bytes(f.get(fuel::STORAGE_OFFSET..fuel::STORAGE_OFFSET + 2)?.try_into().ok()?) as usize;
        let slots = [at, at + 4];
        if slots[1] + 4 > f.len() {
            return None;
        }
        let values = slots.map(|s| u32_at(&f, s));
        let (fuel, max) = (u16::from_le_bytes([n[0], n[1]]) as u32, u16::from_le_bytes([n[2], n[3]]) as u32);
        let amount = match values {
            // a full tank does not tell; the order is the same for every tank
            [a, b] if a == b && a == fuel && b == max => AMOUNT_INDEX.load(Ordering::Relaxed) as usize,
            [a, m] if a == fuel && m == max => 0,
            [m, a] if a == fuel && m == max => 1,
            _ => return None,
        };
        if amount > 1 {
            return None;
        }
        if values[0] != values[1] {
            AMOUNT_INDEX.store(amount as u8, Ordering::Relaxed);
        }
        Some((f, slots, amount))
    }
}

impl Base for WorldBase<'_> {
    fn npcs_near(&self, entity: u32, radius: f64) -> Vec<(u32, u32)> {
        self.world
            .entities_near(entity, radius, "ActiveNpcState")
            .into_iter()
            .filter_map(|npc| Some((npc, u32_at(&self.world.component(npc, "Crafting")?, 0x10))))
            .filter(|&(_, workshop)| workshop != 0)
            .collect()
    }

    fn stations_near(&self, entity: u32, radius: f64) -> Vec<Station> {
        self.world
            .entities_near(entity, radius, "FactoryStation")
            .into_iter()
            .filter_map(|e| {
                let f = self.world.component(e, "FactoryStation")?;
                let workshop = u32_at(&self.world.component(e, "Crafting")?, 0x10);
                Some(Station {
                    entity: e,
                    workshop,
                    state: *f.get(factory::STATE)?,
                    running: u32_at(&f, factory::RUNNING),
                    next: u32_at(&f, factory::NEXT),
                    selections: (0..factory::SELECTIONS)
                        .map(|i| factory::NEXT_SELECTIONS + i * factory::SELECTION_SIZE)
                        .filter(|&at| f.len() > at + 4)
                        .map(|at| (u32_at(&f, at), f[at + 4]))
                        .filter(|&(id, _)| id != 0)
                        .collect(),
                    missing: *f.get(factory::MISSING)?,
                })
            })
            .collect()
    }

    fn chests_near(&self, entity: u32, radius: f64) -> Vec<u32> {
        // stations, NPCs, animals, crops, loot plants and loot containers have inventories too
        // (animal feeders are `AnimalTarget`s: their food is not stock to take or fill)
        const NOT_STORAGE: [&str; 8] = ["FactoryStation", "NpcCommon", "ActiveNpcState", "DestroyOnLoot", "InteractionLootRefill", "Growth", "RandomLootContainer", "AnimalTarget"];
        let not_storage = |e: u32| NOT_STORAGE.iter().any(|c| self.world.component(e, c).is_some());
        self.world.inventories_near(entity, radius).into_iter().filter(|&e| !not_storage(e)).collect()
    }

    fn slots(&self, entity: u32) -> Vec<Slot> {
        self.world.inventory(entity)
    }

    fn move_items(&self, from: u32, slot: u64, to: u32, count: u32) -> u32 {
        if count == u32::MAX {
            self.world.move_stack(from, slot, to)
        } else {
            self.world.move_items(from, slot, to, count)
        }
    }

    fn set_recipe(&self, station: u32, recipe: u32, selections: &[(u32, u8)]) -> bool {
        let Some(mut f) = self.world.component(station, "FactoryStation") else { return false };
        if f.len() < factory::MISSING + 1 {
            return false;
        }
        f[factory::NEXT..factory::NEXT + 4].copy_from_slice(&recipe.to_le_bytes());
        for i in 0..factory::SELECTIONS {
            let at = factory::NEXT_SELECTIONS + i * factory::SELECTION_SIZE;
            let (id, index) = selections.get(i).copied().unwrap_or((0, 0));
            f[at..at + 4].copy_from_slice(&id.to_le_bytes());
            f[at + 4] = index;
        }
        // the station may switch to another option of a category when ours runs out
        if recipe != 0 {
            f[factory::USE_ALL_OPTIONS] = 1;
        }
        f[factory::STOPPED_BY_PLAYER] = 0;
        self.world.set_component(station, "FactoryStation", &f)
    }

    fn tank(&self, entity: u32) -> Option<Tank> {
        let Some((f, slots, amount)) = self.fuel_layout(entity) else {
            // full, before any tank showed the order: amount and maximum are equal
            let n = self.world.component(entity, "NetworkFuel")?;
            let (fuel, max) = (u16::from_le_bytes([n[0], n[1]]) as u32, u16::from_le_bytes([n[2], n[3]]) as u32);
            return (fuel == max && self.world.component(entity, "Fuel").is_some()).then_some(Tank { amount: fuel, max });
        };
        Some(Tank { amount: u32_at(&f, slots[amount]), max: u32_at(&f, slots[1 - amount]) })
    }

    fn set_tank(&self, entity: u32, value: u32) -> bool {
        let Some((mut f, slots, amount)) = self.fuel_layout(entity) else { return false };
        let value = value.min(u32_at(&f, slots[1 - amount]));
        f[slots[amount]..slots[amount] + 4].copy_from_slice(&value.to_le_bytes());
        self.world.set_component(entity, "Fuel", &f)
    }

    fn wells_near(&self, entity: u32, radius: f64) -> Vec<u32> {
        self.world.entities_near(entity, radius, "FuelCraftingStock")
    }

    fn harvestables_near(&self, entity: u32, radius: f64) -> Vec<(u32, Harvest)> {
        // a crop counts when a growing plant (`ecs::Growth`) stands where it is: its root;
        // other loot that vanishes when looted (bags, wild scatter loot) is left alone
        let roots: Vec<[f64; 3]> = self.world.entities_near(entity, radius + 2.0, "Growth").into_iter().filter_map(|r| self.world.position(r)).collect();
        let near_root = |p: [f64; 3]| roots.iter().any(|r| (r[0] - p[0]).powi(2) + (r[1] - p[1]).powi(2) + (r[2] - p[2]).powi(2) < 2.25);
        // and only a ripe stage: one whose harvest the crop table knows (seedling stages
        // have `DestroyOnLoot` too, but no harvest of their own)
        let ripe = |c: u32| self.crop_seedling(c).is_some();
        let crops = self.world.entities_near(entity, radius, "DestroyOnLoot").into_iter().filter(|&c| self.world.position(c).is_some_and(near_root) && ripe(c)).map(|c| (c, Harvest::Crop));
        // refilling loot of the players' own: planted trees are crafting stock, animals tame
        let mine = |e: u32| self.world.component(e, "InventoryCraftingStock").is_some() || self.world.component(e, "IsTame").is_some();
        let refills = self.world.entities_near(entity, radius, "InteractionLootRefill").into_iter().filter(|&e| mine(e)).map(|e| (e, Harvest::Refill));
        crops.chain(refills).collect()
    }

    fn growing_near(&self, entity: u32, radius: f64) -> Vec<[f64; 3]> {
        let refills: Vec<[f64; 3]> = self.world.entities_near(entity, radius + 2.0, "InteractionLootRefill").into_iter().filter_map(|e| self.world.position(e)).collect();
        let is_refill = |p: [f64; 3]| refills.iter().any(|r| (r[0] - p[0]).powi(2) + (r[1] - p[1]).powi(2) + (r[2] - p[2]).powi(2) < 0.36);
        self.world.entities_near(entity, radius, "Growth").into_iter().filter_map(|r| self.world.position(r)).filter(|&p| !is_refill(p)).collect()
    }

    fn finish_crop(&self, crop: u32) -> bool {
        let Some(mut d) = self.world.component(crop, "DestroyOnLoot") else { return false };
        if d.len() <= destroy_on_loot::SHOULD_DESTROY || d[destroy_on_loot::IS_ACTIVE] == 0 || d[destroy_on_loot::SHOULD_DESTROY] != 0 || self.world.inventory(crop).iter().any(|s| s.count != 0) {
            return false;
        }
        d[destroy_on_loot::SHOULD_DESTROY] = 1;
        self.world.set_component(crop, "DestroyOnLoot", &d)
    }

    fn give(&self, entity: u32, item: u32, count: u32) -> u32 {
        self.world.give_items(entity, item, count)
    }

    fn remove(&self, entity: u32, slot: u64, count: u32) -> u32 {
        self.world.remove_items(entity, slot, count)
    }

    fn crop_seedling(&self, crop: u32) -> Option<u32> {
        let inventory = crate::fishing::guid_text(&self.world.component(crop, "DefaultInventory")?)?;
        crate::plants::crops().get(&inventory).map(|c| c.seedling)
    }

    fn crop_loot(&self, crop: u32) -> Option<Vec<(u32, u32)>> {
        let d = self.world.component(crop, "DefaultInventory")?;
        let harvest = &crate::plants::crops().get(&crate::fishing::guid_text(&d)?)?.harvest;
        (*d.get(default_inventory::APPLIED)? == 0 && !harvest.is_empty()).then(|| harvest.clone())
    }

    fn crops_near(&self, entity: u32, radius: f64) -> Vec<(u32, u32, bool)> {
        let roots: Vec<[f64; 3]> = self.world.entities_near(entity, radius + 2.0, "Growth").into_iter().filter_map(|r| self.world.position(r)).collect();
        let near_root = |p: [f64; 3]| roots.iter().any(|r| (r[0] - p[0]).powi(2) + (r[1] - p[1]).powi(2) + (r[2] - p[2]).powi(2) < 2.25);
        self.world
            .entities_near(entity, radius, "DefaultInventory")
            .into_iter()
            .filter_map(|e| {
                let guid = crate::fishing::guid_text(&self.world.component(e, "DefaultInventory")?)?;
                let (crop, ripe) = crate::plants::crop_of(&guid)?;
                self.world.position(e).is_some_and(near_root).then_some((e, crop.seedling, ripe))
            })
            .collect()
    }

    fn crop_picked(&self, crop: u32) -> bool {
        let Some(d) = self.world.component(crop, "DefaultInventory") else { return false };
        let loot_crop = crate::fishing::guid_text(&d).and_then(|g| crate::plants::crops().get(&g).map(|c| !c.harvest.is_empty())) == Some(true);
        loot_crop && d.get(default_inventory::APPLIED) == Some(&1) && self.world.inventory(crop).iter().all(|s| s.count == 0)
    }

    fn mark_looted(&self, crop: u32) -> bool {
        let Some(mut d) = self.world.component(crop, "DefaultInventory") else { return false };
        if d.len() <= default_inventory::APPLIED {
            return false;
        }
        d[default_inventory::APPLIED] = 1;
        self.world.set_component(crop, "DefaultInventory", &d)
    }

    fn replant(&self, crop: u32) -> bool {
        // the plant root stands where its stage is
        let Some(at) = self.world.position(crop) else { return false };
        let dist = |e: u32| self.world.position(e).map_or(f64::MAX, |p| (p[0] - at[0]).powi(2) + (p[1] - at[1]).powi(2) + (p[2] - at[2]).powi(2));
        let Some(root) = self.world.entities_near(crop, 1.5, "Growth").into_iter().min_by(|a, b| dist(*a).total_cmp(&dist(*b))) else { return false };
        let (Some(mut l), Some(n)) = (self.world.component(root, "Level"), self.world.component(root, "NetworkLevel")) else { return false };
        if l.get(level::FLAGS).is_none_or(|f| f & 1 == 0) {
            return false;
        }
        let at = u16::from_le_bytes([l[level::STORAGE_OFFSET], l[level::STORAGE_OFFSET + 1]]) as usize;
        if at + 8 > l.len() {
            return false;
        }
        let current = u16::from_le_bytes([n[0], n[1]]) as u32;
        let values = [u32_at(&l, at), u32_at(&l, at + 4)];
        // the level is the value NetworkLevel shows; the other one (the maximum) must differ
        let index = match values {
            [a, b] if a == current && b != current => 0,
            [a, b] if b == current && a != current => 1,
            _ => return false,
        };
        if current == 0 {
            return false;
        }
        // the upgrade system swaps stages when the level changes, and leaves a loot stage
        // not yet marked looted standing (seen on beans: the root at level 1 under a ripe
        // stage): marked first, the swap takes it away
        self.finish_crop(crop);
        l[at + index * 4..at + index * 4 + 4].copy_from_slice(&0u32.to_le_bytes());
        self.world.set_component(root, "Level", &l)
    }

    fn position(&self, entity: u32) -> Option<[f64; 3]> {
        self.world.position(entity)
    }

    fn is_assistant(&self, npc: u32) -> bool {
        self.world.component(npc, "InventorySetup").is_some()
    }

    fn npc_home(&self, npc: u32) -> Option<Home> {
        let d = self.world.component(npc, "ActiveNpcStateSaved")?;
        let f = |at: usize| d.get(at..at + 4).map(|b| f32::from_le_bytes(b.try_into().unwrap()));
        Some(Home {
            pos: [f(npc_saved::SPAWN)?, f(npc_saved::SPAWN + 4)?, f(npc_saved::SPAWN + 8)?],
            override_range: *d.get(npc_saved::OVERRIDE_RANGE)? != 0,
            range: f(npc_saved::RANGE)?,
        })
    }

    fn set_npc_home(&self, npc: u32, home: Home) -> bool {
        let Some(mut d) = self.world.component(npc, "ActiveNpcStateSaved") else { return false };
        if d.len() < npc_saved::RANGE + 4 {
            return false;
        }
        for (i, v) in home.pos.iter().enumerate() {
            d[npc_saved::SPAWN + i * 4..npc_saved::SPAWN + i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        d[npc_saved::OVERRIDE_RANGE] = home.override_range as u8;
        d[npc_saved::RANGE..npc_saved::RANGE + 4].copy_from_slice(&home.range.to_le_bytes());
        self.world.set_component(npc, "ActiveNpcStateSaved", &d)
    }

    fn nudge_npc(&self, npc: u32) -> bool {
        let Some(mut d) = self.world.component(npc, "ActiveNpcState") else { return false };
        if !matches!(d.first(), Some(&(npc_state::IDLE | npc_state::ROAM | npc_state::WAITING_CLOSE_TO_PLAYER))) {
            return false;
        }
        d[0] = npc_state::DECIDE_TASK;
        self.world.set_component(npc, "ActiveNpcState", &d)
    }

    fn work_at_station(&self, npc: u32) -> bool {
        let Some(mut d) = self.world.component(npc, "ActiveNpcState") else { return false };
        if d.len() < 3 || !matches!(d[0], npc_state::IDLE | npc_state::ROAM | npc_state::WAITING_CLOSE_TO_PLAYER) {
            return false;
        }
        // the live state (`DynamicActiveNpcState`): +0x0 state, +0x2 decision
        d[2] = npc_decision::WANTS_TO_INTERACT;
        d[0] = npc_state::FIND_TARGET;
        self.world.set_component(npc, "ActiveNpcState", &d)
    }

    fn feeders_near(&self, entity: u32, radius: f64) -> Vec<(u32, u32)> {
        let feeding = crate::feeding::data();
        let feeder = |e: u32| {
            let item = u32_at(&self.world.component(e, "UsedItem")?, 0);
            feeding.food_of(item).map(|_| (e, item))
        };
        match self.base_zone(entity) {
            Some((altar, center, half)) => {
                let inside = |e: u32| self.world.position(e).is_some_and(|p| (0..3).all(|i| (p[i] - center[i]).abs() <= half));
                self.world.entities_near(altar, half * 1.75, "AnimalTarget").into_iter().filter(|&e| inside(e)).filter_map(feeder).collect()
            }
            None => self.world.entities_near(entity, radius, "AnimalTarget").into_iter().filter_map(feeder).collect(),
        }
    }

    fn fishing_spot_near(&self, entity: u32, radius: f64) -> Option<String> {
        let spot = self.world.entities_near(entity, radius, "FishingSpot").into_iter().next()?;
        crate::fishing::guid_text(&self.world.component(spot, "FishingSpot")?)
    }
}
