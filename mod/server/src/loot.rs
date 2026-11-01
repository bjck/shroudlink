//! Better loot for mods (HostApi v9 `set_loot_boost`): every loot roll of the game (enemy
//! drops, containers, world pickups) starts from a generator state built by
//! `FUN_140166d60(out, gen, lootSys, defaults, req)`: +0x3c0 level (f32, from the request,
//! 0 when it has none), +0x3c4 level offset, +0x3c8 / +0x3cc random range, +0x3ec five
//! rarity weights (f32, common to legendary, from `globalRarityDropRates`). A post-hook
//! lifts the level and scales the weights while a mod asks for it. Each item clamps the
//! rolled level to its own `itemLevelRange` (most gear: 55), so levels past that need a
//! data patch. Crafting does not use this generator.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use chestsort_common::log;
use retour::GenericDetour;

use crate::hooks;

type InitFn = unsafe extern "system" fn(out: *mut u8, gen: *mut u8, system: usize, defaults: usize, request: usize) -> *mut u8;

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
static BOOST: Mutex<Option<Boost>> = Mutex::new(None);
/// rolls changed so far
static BOOSTED: AtomicU64 = AtomicU64::new(0);

pub fn install() -> Result<(), String> {
    unsafe {
        let target = std::mem::transmute::<usize, InitFn>(hooks::address("loot_gen_init")?);
        let detour = GenericDetour::<InitFn>::new(target, init_hook).map_err(|e| e.to_string())?;
        INIT.get_or_init(|| detour).enable().map_err(|e| e.to_string())?;
    }
    Ok(())
}

unsafe extern "system" fn init_hook(out: *mut u8, gen: *mut u8, system: usize, defaults: usize, request: usize) -> *mut u8 {
    let r = INIT.get().unwrap().call(out, gen, system, defaults, request);
    if let (Some(b), false) = (*BOOST.lock().unwrap(), gen.is_null()) {
        let level = gen.add(GEN_LEVEL) as *mut f32;
        *level = level.read().max(b.min_level) + b.add_levels;
        let weights = gen.add(GEN_WEIGHTS) as *mut f32;
        for (i, m) in b.rarity.iter().enumerate() {
            *weights.add(i) *= m;
        }
        BOOSTED.fetch_add(1, Ordering::Relaxed);
    }
    r
}

/// Sets the boost every following roll gets (None: the game's own loot).
pub fn set(boost: Option<Boost>) {
    let mut current = BOOST.lock().unwrap();
    if *current != boost {
        log!("loot: boost {boost:?} ({} roll(s) boosted so far)", BOOSTED.load(Ordering::Relaxed));
        *current = boost;
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
