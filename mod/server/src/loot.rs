//! Better loot for mods (HostApi v9 `set_loot_boost`): every loot roll of the game (enemy
//! drops, containers, world pickups) starts from a generator state built by
//! `FUN_140166d60(out, gen, lootSys, defaults, req)`: +0x3c0 level (f32, from the request,
//! 0 when it has none), +0x3c4 level offset, +0x3c8 / +0x3cc random range, +0x3ec five
//! rarity weights (f32, common to legendary, from `globalRarityDropRates`). A post-hook
//! scales the rarity weights while a mod asks for it. Every slot is then rolled by
//! `FUN_140166180(slot, item, gen, rarity)` (+0 item, +4 count u16, +6 rarity, +8 level
//! u16): a post-hook lifts that level, so it holds whatever set the generator's level.
//! The item's creation clamps it to the item's own `itemLevelRange` (Level_Max: an item of
//! 1-10 stays at most 10; most late gear stops at 55); which items are picked depends on
//! the loot table, not on the level. Crafting does not use this generator.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use chestsort_common::log;
use retour::GenericDetour;

use crate::hooks;

type InitFn = unsafe extern "system" fn(out: *mut u8, gen: *mut u8, system: usize, defaults: usize, request: usize) -> *mut u8;
type SlotFn = unsafe extern "system" fn(slot: *mut u8, item: *const u32, gen: *mut u8, rarity: *mut u8);

const GEN_LEVEL: usize = 0x3c0;
const GEN_WEIGHTS: usize = 0x3ec;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Boost {
    /// the level a roll has at least
    pub min_level: f32,
    /// levels added on top
    pub add_levels: f32,
    /// multipliers of the five rarity weights (common … legendary)
    pub rarity: [f32; 5],
}

static INIT: OnceLock<GenericDetour<InitFn>> = OnceLock::new();
static SLOT: OnceLock<GenericDetour<SlotFn>> = OnceLock::new();
/// slot rolls still logged after the last boost change (to see what a boost does)
static LOG_BUDGET: AtomicU64 = AtomicU64::new(40);
const SLOT_LEVEL: usize = 8;
static BOOST: Mutex<Option<Boost>> = Mutex::new(None);
/// rolls changed so far
static BOOSTED: AtomicU64 = AtomicU64::new(0);

pub fn install() -> Result<(), String> {
    unsafe {
        let target = std::mem::transmute::<usize, InitFn>(hooks::address("loot_gen_init")?);
        let detour = GenericDetour::<InitFn>::new(target, init_hook).map_err(|e| e.to_string())?;
        INIT.get_or_init(|| detour).enable().map_err(|e| e.to_string())?;
        let target = std::mem::transmute::<usize, SlotFn>(hooks::address("loot_slot")?);
        let detour = GenericDetour::<SlotFn>::new(target, slot_hook).map_err(|e| e.to_string())?;
        SLOT.get_or_init(|| detour).enable().map_err(|e| e.to_string())?;
    }
    Ok(())
}

unsafe extern "system" fn init_hook(out: *mut u8, gen: *mut u8, system: usize, defaults: usize, request: usize) -> *mut u8 {
    let r = INIT.get().unwrap().call(out, gen, system, defaults, request);
    // the level is lifted per slot (slot_hook); here only the rarity weights
    if let (Some(b), false) = (*BOOST.lock().unwrap(), gen.is_null()) {
        let weights = gen.add(GEN_WEIGHTS) as *mut f32;
        for (i, m) in b.rarity.iter().enumerate() {
            *weights.add(i) *= m;
        }
        BOOSTED.fetch_add(1, Ordering::Relaxed);
    }
    r
}

unsafe extern "system" fn slot_hook(slot: *mut u8, item: *const u32, gen: *mut u8, rarity: *mut u8) {
    SLOT.get().unwrap().call(slot, item, gen, rarity);
    let Some(b) = *BOOST.lock().unwrap() else { return };
    if slot.is_null() {
        return;
    }
    let level = slot.add(SLOT_LEVEL) as *mut u16;
    let rolled = level.read();
    let lifted = (rolled as f32).max(b.min_level) + b.add_levels;
    *level = lifted.clamp(0.0, u16::MAX as f32) as u16;
    if LOG_BUDGET.load(Ordering::Relaxed) > 0 {
        LOG_BUDGET.fetch_sub(1, Ordering::Relaxed);
        log!(
            "loot: slot item {} x{} rarity {} level {rolled} -> {} (generator level {})",
            *(slot as *const u32),
            *(slot.add(4) as *const u16),
            *slot.add(6),
            level.read(),
            if gen.is_null() { 0.0 } else { *(gen.add(GEN_LEVEL) as *const f32) }
        );
    }
}

/// Sets the boost every following roll gets (None: the game's own loot).
pub fn set(boost: Option<Boost>) {
    let mut current = BOOST.lock().unwrap();
    if *current != boost {
        log!("loot: boost {boost:?} ({} roll(s) boosted so far)", BOOSTED.load(Ordering::Relaxed));
        *current = boost;
        LOG_BUDGET.store(40, Ordering::Relaxed);
    }
}

pub fn boosted() -> u64 {
    BOOSTED.load(Ordering::Relaxed)
}

pub unsafe extern "C" fn api_set_loot_boost(min_level: f64, add_levels: f64, rarity: *const f64) -> bool {
    if !hooks::available(hooks::Feature::Loot) {
        return false;
    }
    let mut weights = [1f32; 5];
    if !rarity.is_null() {
        for (i, w) in weights.iter_mut().enumerate() {
            let m = *rarity.add(i);
            *w = if m.is_finite() { m.clamp(0.0, 100.0) as f32 } else { 1.0 };
        }
    }
    let (min_level, add_levels) = (min_level.clamp(0.0, 255.0) as f32, add_levels.clamp(-255.0, 255.0) as f32);
    let none = min_level == 0.0 && add_levels == 0.0 && weights == [1.0; 5];
    set((!none).then_some(Boost { min_level, add_levels, rarity: weights }));
    true
}
