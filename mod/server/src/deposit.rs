//! Hook on the per-chest step of the Quick Stack Station area deposit.
//!
//! Vanilla only pulls items the chest already contains. If the chest was
//! renamed to one or more category words ("Food", "Ores & Gems"), we first move
//! every player stack of those categories into it, then let vanilla run as usual.

use std::collections::HashSet;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::OnceLock;

use chestsort_common::log;
use retour::GenericDetour;

use crate::game::{slot_id, Ctx, DepositIntoChestFn, DepositResult, Game, MoveAllFn, Page, SLOTS_PER_PAGE};
use crate::{categories, names};

static GAME: OnceLock<Game> = OnceLock::new();
static HOOK: OnceLock<GenericDetour<DepositIntoChestFn>> = OnceLock::new();
static MOVE_ALL_HOOK: OnceLock<GenericDetour<MoveAllFn>> = OnceLock::new();

/// Component type index of `keen::ecs::CustomPlayerString`, learned on first match.
static NAME_COMPONENT: AtomicU32 = AtomicU32::new(u32::MAX);
static LAYOUT_CHECKED: AtomicBool = AtomicBool::new(false);
static LAYOUT_OK: AtomicBool = AtomicBool::new(false);

pub fn install(game: Game) -> Result<(), String> {
    let target = game.deposit_into_chest;
    let _ = GAME.set(game);
    let hook = unsafe { GenericDetour::<DepositIntoChestFn>::new(target, deposit_into_chest_hook) }
        .map_err(|e| format!("creating hook failed: {e}"))?;
    unsafe { hook.enable() }.map_err(|e| format!("enabling hook failed: {e}"))?;
    let _ = HOOK.set(hook);
    let hook = unsafe { GenericDetour::<MoveAllFn>::new(GAME.get().unwrap().move_all, move_all_hook) }
        .map_err(|e| format!("creating move-all hook failed: {e}"))?;
    unsafe { hook.enable() }.map_err(|e| format!("enabling move-all hook failed: {e}"))?;
    let _ = MOVE_ALL_HOOK.set(hook);
    Ok(())
}

/// "Move all" from the player into a chest: locked stacks stay behind.
unsafe extern "system" fn move_all_hook(
    out: *mut DepositResult,
    ctx: *mut Ctx,
    source: u32,
    target: u32,
    mode: u32,
    flag: u8,
) -> *mut DepositResult {
    let original = MOVE_ALL_HOOK.get().unwrap();
    let game = GAME.get().unwrap();
    // only a player's own pages can be matched to the addon's locks
    let locked = locked_slots(game, ctx, source);
    let hidden = hide_locked(game, ctx, source, &locked);
    let result = original.call(out, ctx, source, target, mode, flag);
    for (stack, count) in hidden {
        (*stack).count = count;
    }
    result
}

unsafe extern "system" fn deposit_into_chest_hook(
    out: *mut DepositResult,
    ctx: *mut Ctx,
    chest: u32,
    player: u32,
    feedback: *mut c_void,
) -> *mut DepositResult {
    let original = HOOK.get().unwrap();
    let game = GAME.get().unwrap();

    let locked = locked_slots(game, ctx, player);
    let mut sorted = 0;
    if let Some(name) = chest_name(game, ctx, chest) {
        let items = categories::items_for(&name);
        if items.is_empty() {
            log!("chest {chest} '{name}': no category in name, vanilla only");
        } else {
            sorted = deposit_filtered(game, ctx, chest, player, &items, &locked);
            log!("chest {chest} '{name}' {:?}: moved {sorted} item(s) by category", categories::parse(&name));
        }
    }

    // the vanilla deposit knows nothing about locks: it skips empty slots, so locked
    // stacks are shown to it as empty for the duration of the call
    let hidden = hide_locked(game, ctx, player, &locked);
    let result = original.call(out, ctx, chest, player, feedback);
    for (stack, count) in hidden {
        (*stack).count = count;
    }
    if !result.is_null() && (*result).error == 0 {
        (*result).moved += sorted;
    }
    result
}

/// The player's slots locked with the client addon.
unsafe fn locked_slots(game: &Game, ctx: *mut Ctx, player: u32) -> HashSet<u64> {
    // the quick stack also runs from chest to player; only a player's own slots count
    if !crate::locks::any() || !game.is_player(ctx, player) {
        return HashSet::new();
    }
    let Some(setup) = game.inventory_setup(ctx, player) else { return HashSet::new() };
    let pages: Vec<u32> = game.pages(ctx, setup).pages().iter().map(|p| p.entity).filter(|&e| e != 0).collect();
    let locked = crate::locks::locked_in(&pages);
    log!("player {player}: {} locked slot(s) kept", locked.len());
    locked
}

/// Empties the player's locked stacks; returns them with their counts for restoring.
unsafe fn hide_locked(game: &Game, ctx: *mut Ctx, player: u32, locked: &HashSet<u64>) -> Vec<(*mut crate::game::ItemStack, u32)> {
    let mut hidden = Vec::new();
    if locked.is_empty() {
        return hidden;
    }
    let Some(setup) = game.inventory_setup(ctx, player) else { return hidden };
    for page in game.pages(ctx, setup).pages().iter().filter(|p| p.entity != 0) {
        for slot in 0..SLOTS_PER_PAGE {
            let stack = page.slots.add(slot as usize);
            if (*stack).count != 0 && locked.contains(&slot_id(page.entity, slot)) {
                hidden.push((stack, (*stack).count));
                (*stack).count = 0;
            }
        }
    }
    hidden
}

/// The name given to the chest with the Rename button, if any.
unsafe fn chest_name(game: &Game, ctx: *mut Ctx, chest: u32) -> Option<String> {
    if !LAYOUT_CHECKED.swap(true, Ordering::Relaxed) {
        let ok = game.verify_component_layout(ctx, chest);
        LAYOUT_OK.store(ok, Ordering::Relaxed);
        log!("component layout check: {}", if ok { "ok" } else { "FAILED - chest names disabled" });
    }
    if !LAYOUT_OK.load(Ordering::Relaxed) {
        return None;
    }
    let record = game.entity_record(ctx, chest)?;
    let known = NAME_COMPONENT.load(Ordering::Relaxed);
    if known != u32::MAX {
        let (addr, size) = record.component(known as u16)?;
        return (size == 8).then(|| names::lookup_fresh(*(addr as *const u64))).flatten();
    }
    // not learned yet: the name component is the 8-byte component whose value is a name key
    for (index, addr) in record.components_of_size(8) {
        if let Some(name) = names::lookup(*(addr as *const u64)) {
            NAME_COMPONENT.store(index as u32, Ordering::Relaxed);
            log!("chest name component is type #{index}");
            return Some(name);
        }
    }
    None
}

/// Moves all player stacks whose item id is in `items` into the chest.
/// Returns the number of items moved.
unsafe fn deposit_filtered(game: &Game, ctx: *mut Ctx, chest: u32, player: u32, items: &HashSet<u32>, locked: &HashSet<u64>) -> u32 {
    if game.deposit_disabled(ctx) {
        return 0;
    }
    let (Some(chest_setup), Some(player_setup)) = (game.inventory_setup(ctx, chest), game.inventory_setup(ctx, player)) else {
        return 0;
    };
    let chest_pages = game.pages(ctx, chest_setup);
    let player_pages = game.pages(ctx, player_setup);
    let reserved = game.player_reserved(ctx, player);

    let mut moved = 0;
    for src_page in player_pages.pages().iter().filter(|p| p.entity != 0 && !reserved.pages.contains(&p.entity)) {
        for src_slot in 0..SLOTS_PER_PAGE {
            let stack = &*src_page.slots.add(src_slot as usize);
            if stack.count == 0 || !items.contains(&stack.item_id) {
                continue;
            }
            // equipped, in hand, or locked by the player: stays where it is
            let id = slot_id(src_page.entity, src_slot);
            if reserved.slots.contains(&id) || locked.contains(&id) {
                continue;
            }
            // unlike vanilla quick stacking, items with their own entity data (gear, weapon
            // gems) are moved too: the transfer carries the PIDE along with the slot
            let Some(info) = game.item_info(ctx, stack.item_id) else { continue };
            // keep going until the source stack is empty or the chest has no room
            while stack.count != 0 {
                let Some(dst) = find_destination(chest_pages.pages(), stack.item_id, info.max_stack as u32) else {
                    return moved;
                };
                let result = game.transfer(ctx, dst, slot_id(src_page.entity, src_slot));
                if result.error != 0 || result.moved == 0 {
                    break;
                }
                moved += result.moved;
            }
        }
    }
    moved
}

/// A partial stack of the same item, otherwise the first empty slot.
unsafe fn find_destination(pages: &[Page], item_id: u32, max_stack: u32) -> Option<u64> {
    let mut empty = None;
    for page in pages.iter().filter(|p| p.entity != 0) {
        for slot in 0..SLOTS_PER_PAGE {
            let stack = &*page.slots.add(slot as usize);
            if stack.count == 0 {
                empty.get_or_insert(slot_id(page.entity, slot));
            } else if stack.item_id == item_id && stack.count < max_stack {
                return Some(slot_id(page.entity, slot));
            }
        }
    }
    empty
}
