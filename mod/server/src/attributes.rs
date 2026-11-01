//! Attributes for mods (HostApi v9): an entity's Level, Strength_Base, Health_Max_Adder,
//! DamageMod_Multiplier and every other attribute slot, read and written the way the
//! game's own systems do it.
//!
//! An attribute group is a component named like it (`Level`, `Strength`, `Health` …):
//! header +0 root id (u32), +4 storage offset (u16), +6 flags (bit 0: initialized), +7
//! slot count; slot `i` is a u32, s32 or f32 at `comp + offset + i * 4`. Slots are found
//! by id (the hash of their name, `shroudlink_sdk::attr`) in the game's attribute
//! registry (`impact::AttributeRegistry`): `FUN_140986f90(out, id, registry)` gives a
//! 32-byte entry, +0 root id, +4 slot id, +8 value type, +0xc slot index (u16), +0x10
//! name (pointer, length at +0x18).
//!
//! Slots are tied together by formulas (Strength = Strength_Base + Strength_Flame_Base,
//! Level = clamp(Level, 0, Level_Max)), so after a write the component is recomputed:
//! u32 and float through the game's setters (`FUN_140197fa0`, `FUN_140111ef0`: write the
//! slot, then the typed recompute), s32 by writing the slot and calling the s32
//! recompute, which has no setter of its own. The u32 and s32 recompute functions are
//! the same template with a different evaluator (`FUN_1400a7c80` / `FUN_14004f140`):
//! [`RECALC_TEMPLATE`] matches both, the u32 one is the setter's call, the s32 one the other.
//!
//! No world context leads to the registry, but the game passes it to every lookup (also
//! without players, from the first second): a detour on the lookup remembers it.
//!
//! Only Level and Experience are saved with a character, and every spawn checks them
//! against the server's rules; the core refuses to write them for players. Everything
//! else is rebuilt at each spawn, so a mod re-applies what it adds.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use chestsort_common::log;
use chestsort_common::scan::{call_targets, Module};
use retour::GenericDetour;

use crate::{components, events, hooks, world_ctx};

type LookupFn = unsafe extern "system" fn(out: *mut u8, id: u32, registry: *mut u8) -> *mut u8;
type SetU32Fn = unsafe extern "system" fn(comp: *mut u8, registry: *mut u8, value: u32, id: u32);
type SetF32Fn = unsafe extern "system" fn(comp: *mut u8, registry: *mut u8, value: f32, id: u32);
type RecalcFn = unsafe extern "system" fn(comp: *mut u8, registry: *mut u8);

/// `FUN_1400a7c80` / `FUN_14004f140` up to the evaluator call (the u32 and s32 recompute).
const RECALC_TEMPLATE: &str = "40 56 48 83 EC 60 4C 8B C2 48 8B F1 8B 11 48 8D 4C 24 30 E8 ?? ?? ?? ?? 80 7C 24 30 00 0F 85 D6 00 00 00 48 8B 4C 24 38 48 89 5C 24 70 8B 41 10 83 E8 01 48 63 D8 0F 88 B8 00 00 00 48 89 7C 24 78 48 8B FB 4C 89 B4 24 80 00 00 00 4C 8D 71 0C 48 C1 E7 04 0F 1F 40 00 0F 1F 84 00 00 00 00 00 49 63 0E 33 D2 85 C9 49 8D 04 0E 48 0F 45 D0 8B 4C 3A 0C 85 C9 74 62 48 63 44 3A 08 85 C0 75 04 33 C0 EB 0A 48 83 C0 08 48 03 C7 48 03 C2 48 89 44 24 40 4C 8D 44 24 40 0F B7 46 04 48 8D 54 24 50 48 03 C6 48 89 4C 24 48 48 89 44 24 50 48 8D 4C 24 20 0F B6 46 07 45 33 C9 48 89 44 24 58 E8";

/// Value types in a registry entry (+8).
const TYPE_U32: u32 = 0x3294_0bec;
const TYPE_S32: u32 = 0x9859_f31e;
const TYPE_F32: u32 = 0xe89f_7410;

/// Root ids of the groups a character save keeps (Level, Experience).
const SAVED_ROOTS: [u32; 2] = [0x724b_f568, 0x8dcd_daf4];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    U32,
    S32,
    F32,
}

/// Where a slot lives, from the registry.
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    pub root: u32,
    /// `qualifiedHash` of the group's component
    pub component: u32,
    pub kind: Kind,
    pub index: u16,
}

struct Fns {
    lookup: GenericDetour<LookupFn>,
    set_u32: SetU32Fn,
    set_f32: SetF32Fn,
    recalc_s32: RecalcFn,
}
unsafe impl Send for Fns {}
unsafe impl Sync for Fns {}

static FNS: OnceLock<Fns> = OnceLock::new();
/// The registry the game passed to its last lookup.
static REGISTRY: AtomicUsize = AtomicUsize::new(0);
static SLOTS: Mutex<Option<HashMap<u32, Option<Slot>>>> = Mutex::new(None);

pub fn install(module: &Module) -> Result<(), String> {
    unsafe {
        let lookup_at = hooks::address("attr_lookup")?;
        let set_u32 = hooks::address("attr_set_u32")?;
        let set_f32 = hooks::address("attr_set_f32")?;
        let recalc_u32 = match call_targets(set_u32, 0x75).as_slice() {
            [first, recalc] if *first == lookup_at => *recalc,
            other => return Err(format!("attr_set_u32 calls {other:x?}, expected the lookup and the recompute")),
        };
        let recalcs = module.find_all(RECALC_TEMPLATE, 3);
        let recalc_s32 = match recalcs.as_slice() {
            [a, b] if *a == recalc_u32 => *b,
            [a, b] if *b == recalc_u32 => *a,
            other => return Err(format!("recompute template found at {other:x?}, u32 one at {recalc_u32:#x}")),
        };
        let lookup = GenericDetour::<LookupFn>::new(std::mem::transmute::<usize, LookupFn>(lookup_at), lookup_hook).map_err(|e| e.to_string())?;
        let fns = FNS.get_or_init(|| Fns {
            lookup,
            set_u32: std::mem::transmute::<usize, SetU32Fn>(set_u32),
            set_f32: std::mem::transmute::<usize, SetF32Fn>(set_f32),
            recalc_s32: std::mem::transmute::<usize, RecalcFn>(recalc_s32),
        });
        fns.lookup.enable().map_err(|e| e.to_string())?;
        log!("attributes: s32 recompute at +{:#x}", module.rva(recalc_s32));
    }
    Ok(())
}

unsafe extern "system" fn lookup_hook(out: *mut u8, id: u32, registry: *mut u8) -> *mut u8 {
    if REGISTRY.load(Ordering::Relaxed) != registry as usize && !registry.is_null() {
        REGISTRY.store(registry as usize, Ordering::Relaxed);
    }
    FNS.get().unwrap().lookup.call(out, id, registry)
}

fn registry() -> Option<*mut u8> {
    let r = REGISTRY.load(Ordering::Relaxed);
    (r != 0 && FNS.get().is_some()).then_some(r as *mut u8)
}

/// The registry entry of a slot id.
unsafe fn entry(registry: *mut u8, id: u32) -> Option<*const u8> {
    let mut out = [0u8; 16];
    FNS.get()?.lookup.call(out.as_mut_ptr(), id, registry);
    let entry = usize::from_le_bytes(out[8..16].try_into().unwrap());
    (out[0] == 0 && entry != 0).then_some(entry as *const u8)
}

unsafe fn name(entry: *const u8) -> Option<String> {
    let text = *(entry.add(0x10) as *const *const u8);
    let len = *(entry.add(0x18) as *const u32) as usize;
    if text.is_null() || len == 0 || len > 64 {
        return None;
    }
    let bytes = std::slice::from_raw_parts(text, len);
    bytes.iter().all(|b| b.is_ascii_graphic()).then(|| String::from_utf8_lossy(bytes).into_owned())
}

/// Where the slot with this id lives; None for an unknown id or before the registry is known.
pub fn slot(id: u32) -> Option<Slot> {
    let registry = registry()?;
    let mut cache = SLOTS.lock().unwrap();
    let cache = cache.get_or_insert_with(HashMap::new);
    if let Some(known) = cache.get(&id) {
        return *known;
    }
    let found = unsafe {
        entry(registry, id).and_then(|e| {
            let root = *(e as *const u32);
            let kind = match *(e.add(8) as *const u32) {
                TYPE_U32 => Kind::U32,
                TYPE_S32 => Kind::S32,
                TYPE_F32 => Kind::F32,
                _ => return None,
            };
            let index = *(e.add(0xc) as *const u16);
            let group = name(entry(registry, root)?)?;
            let component = shroudlink_sdk::components::hash_of(&group)?;
            Some(Slot { root, component, kind, index })
        })
    };
    if found.is_none() {
        log!("attributes: no slot {id:#010x}");
    }
    cache.insert(id, found);
    found
}

/// The entity's component of the slot's group and the slot's address in it.
unsafe fn locate(ctx: *mut crate::game::Ctx, entity: u32, slot: &Slot) -> Option<(*mut u8, *mut u8)> {
    let record = events::game().entity_record(ctx, entity)?;
    let (comp, size) = components::of(ctx, &record, slot.component)?;
    let (root, offset, flags, count) = (*(comp as *const u32), *(comp.add(4) as *const u16) as usize, *comp.add(6), *comp.add(7));
    if root != slot.root || flags & 1 == 0 || slot.index >= count as u16 || offset + (slot.index as usize + 1) * 4 > size as usize {
        return None;
    }
    Some((comp, comp.add(offset + slot.index as usize * 4)))
}

/// An entity's attribute slot (`shroudlink_sdk::attr`), None if it has none.
pub fn get(entity: u32, id: u32) -> Option<f64> {
    let slot = slot(id)?;
    world_ctx::with(|ctx| unsafe {
        let (_, at) = locate(ctx, entity, &slot)?;
        Some(match slot.kind {
            Kind::U32 => *(at as *const u32) as f64,
            Kind::S32 => *(at as *const i32) as f64,
            Kind::F32 => *(at as *const f32) as f64,
        })
    })
    .flatten()
}

/// Writes an entity's attribute slot (rounded and clamped to its type) and lets the game
/// recompute the slots that depend on it. Must run on the game thread.
pub fn set(entity: u32, id: u32, value: f64) -> Result<(), String> {
    let registry = registry().ok_or("the attribute registry is not known yet")?;
    let slot = slot(id).ok_or_else(|| format!("unknown attribute {id:#010x}"))?;
    if !value.is_finite() {
        return Err(format!("value {value}"));
    }
    let fns = FNS.get().ok_or("attributes are off")?;
    world_ctx::with(|ctx| unsafe {
        if SAVED_ROOTS.contains(&slot.root) && events::game().is_player(ctx, entity) {
            return Err("a player's Level and Experience are saved with the character: not written".to_string());
        }
        let (comp, at) = locate(ctx, entity, &slot).ok_or_else(|| format!("entity {entity} has no attribute {id:#010x}"))?;
        match slot.kind {
            Kind::U32 => (fns.set_u32)(comp, registry, value.round().clamp(0.0, u32::MAX as f64) as u32, id),
            Kind::F32 => (fns.set_f32)(comp, registry, value as f32, id),
            Kind::S32 => {
                *(at as *mut i32) = value.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32;
                (fns.recalc_s32)(comp, registry);
            }
        }
        Ok(())
    })
    .unwrap_or_else(|| Err("the world has not loaded yet".to_string()))
}

pub unsafe extern "C" fn api_attribute(entity: u32, id: u32, out: *mut f64) -> bool {
    match get(entity, id) {
        Some(v) if !out.is_null() => {
            *out = v;
            true
        }
        _ => false,
    }
}

pub unsafe extern "C" fn api_set_attribute(entity: u32, id: u32, value: f64) -> bool {
    match set(entity, id, value) {
        Ok(()) => true,
        Err(e) => {
            log!("attributes: set {id:#010x} of {entity} to {value}: {e}");
            false
        }
    }
}

/// For the self-test: is the registry known.
pub fn registry_known() -> bool {
    registry().is_some()
}
