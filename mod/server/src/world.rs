//! World access for mods that is not tied to an inventory event (HostApi v4): the players
//! in the world, positions, teleporting. Runs on a context from [`crate::world_ctx`].
//!
//! Positions: `CurrentTransform.transform.position` is a `keen::WorldPosition`, three
//! `sint64` in metres * 2^32 (y up). A teleport writes the new position into the entity's
//! current and last transform and increments its `TeleportCount`: the counter the client
//! watches to snap to the server's position instead of correcting it (verified in game).

use std::sync::Mutex;

use chestsort_common::log;

use crate::components::{self, hash};
use crate::events;
use crate::world_ctx;

const UNITS_PER_METRE: f64 = 4_294_967_296.0;

/// Addon session -> its player entity, learned from the backpack pages the addon reports.
static CLIENTS: Mutex<Vec<(u64, u32)>> = Mutex::new(Vec::new());

/// An addon reported its backpack pages: find the player whose inventory has them.
pub fn identify(client: u64, pages: &[u32]) -> Option<u32> {
    let player = world_ctx::with(|ctx| unsafe {
        let game = events::game();
        players().into_iter().find(|&p| {
            let theirs: Vec<u32> = game.inventory_setup(ctx, p).map(|setup| game.pages(ctx, setup).pages().iter().map(|page| page.entity).filter(|&e| e != 0).collect()).unwrap_or_default();
            if crate::game_events::probing() {
                log!("world: player {p} has pages {theirs:?}, addon reported {pages:?}");
            }
            theirs.iter().any(|e| pages.contains(e))
        })
    })
    .flatten()?;
    let mut clients = CLIENTS.lock().unwrap();
    clients.retain(|&(c, p)| c != client && p != player);
    clients.push((client, player));
    log!("world: client {client:016x} is player {player}");
    Some(player)
}

pub fn forget_client(client: u64) {
    CLIENTS.lock().unwrap().retain(|&(c, _)| c != client);
}

/// The player entity of an addon session, once it has identified itself.
pub fn player_of(client: u64) -> Option<u32> {
    CLIENTS.lock().unwrap().iter().find(|&&(c, _)| c == client).map(|&(_, p)| p)
}

/// Player entities seen logging in (LoginEvent); pruned when they stop being players.
static PLAYERS: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// A player entered the world (from the game event feed).
pub fn on_login(player: u32) {
    let mut players = PLAYERS.lock().unwrap();
    if !players.contains(&player) {
        players.push(player);
        log!("world: player {player} logged in");
    }
}

pub fn players() -> Vec<u32> {
    let known = PLAYERS.lock().unwrap().clone();
    let alive: Vec<u32> = world_ctx::with(|ctx| unsafe { known.iter().copied().filter(|&p| events::game().is_player(ctx, p)).collect() }).unwrap_or_default();
    *PLAYERS.lock().unwrap() = alive.clone();
    alive
}

/// Position in metres.
pub fn position(entity: u32) -> Option<[f64; 3]> {
    world_ctx::with(|ctx| unsafe {
        let record = events::game().entity_record(ctx, entity)?;
        let (addr, _) = components::of(ctx, &record, hash::CURRENT_TRANSFORM)?;
        let p = addr as *const i64;
        Some([*p as f64 / UNITS_PER_METRE, *p.add(1) as f64 / UNITS_PER_METRE, *p.add(2) as f64 / UNITS_PER_METRE])
    })
    .flatten()
}

/// Inventories within `radius` metres of `around` (loaded entities only): (entity,
/// position, custom name).
pub fn inventories_around(around: [f64; 3], radius: f64) -> Vec<(u32, [f64; 3], Option<String>)> {
    world_ctx::with(|ctx| unsafe {
        let game = events::game();
        let mut found = Vec::new();
        for entity in 1..2_000_000u32 {
            if game.entity_record(ctx, entity).is_none() || game.inventory_setup(ctx, entity).is_none() || game.is_player(ctx, entity) {
                continue;
            }
            let Some(record) = game.entity_record(ctx, entity) else { continue };
            let Some((addr, _)) = components::of(ctx, &record, hash::CURRENT_TRANSFORM) else { continue };
            let p = addr as *const i64;
            let pos = [*p as f64 / UNITS_PER_METRE, *p.add(1) as f64 / UNITS_PER_METRE, *p.add(2) as f64 / UNITS_PER_METRE];
            let d = ((pos[0] - around[0]).powi(2) + (pos[1] - around[1]).powi(2) + (pos[2] - around[2]).powi(2)).sqrt();
            if d <= radius {
                found.push((entity, pos, crate::game_api::name_of(game, ctx, entity)));
            }
        }
        found
    })
    .unwrap_or_default()
}

/// Turns an entity to look along `yaw` radians around the up axis (orientation quaternion
/// of its current and last transform, at +0x18).
pub fn face(entity: u32, yaw: f64) -> bool {
    let q = [0.0f32, (yaw / 2.0).sin() as f32, 0.0, (yaw / 2.0).cos() as f32];
    world_ctx::with(|ctx| unsafe {
        let record = events::game().entity_record(ctx, entity)?;
        for h in [hash::CURRENT_TRANSFORM, hash::LAST_TRANSFORM] {
            if let Some((addr, _)) = components::of(ctx, &record, h) {
                std::ptr::copy_nonoverlapping(q.as_ptr(), addr.add(0x18) as *mut f32, 4);
            }
        }
        Some(())
    })
    .flatten()
    .is_some()
}

/// Moves an entity to `to` (metres); players see themselves there right away.
pub fn teleport(entity: u32, to: [f64; 3]) -> bool {
    let raw = to.map(|v| (v * UNITS_PER_METRE) as i64);
    let moved = world_ctx::with(|ctx| unsafe {
        let record = events::game().entity_record(ctx, entity)?;
        let (current, _) = components::of(ctx, &record, hash::CURRENT_TRANSFORM)?;
        for transform in [Some(current), components::of(ctx, &record, hash::LAST_TRANSFORM).map(|c| c.0)].into_iter().flatten() {
            std::ptr::copy_nonoverlapping(raw.as_ptr(), transform as *mut i64, 3);
        }
        if let Some((count, _)) = components::of(ctx, &record, hash::TELEPORT_COUNT) {
            *count = (*count).wrapping_add(1);
        }
        Some(())
    })
    .flatten()
    .is_some();
    log!("world: teleport {entity} to [{:.1}, {:.1}, {:.1}]: {}", to[0], to[1], to[2], if moved { "done" } else { "failed" });
    moved
}

/// Copies an entity's component of type `type_hash` (its `qualifiedHash`) into `out`;
/// returns the component's size (0: the entity has none).
pub fn read_component(entity: u32, type_hash: u32, out: &mut [u8]) -> usize {
    world_ctx::with(|ctx| unsafe {
        let record = events::game().entity_record(ctx, entity)?;
        let (addr, size) = components::of(ctx, &record, type_hash)?;
        let n = (size as usize).min(out.len());
        std::ptr::copy_nonoverlapping(addr, out.as_mut_ptr(), n);
        Some(size as usize)
    })
    .flatten()
    .unwrap_or(0)
}

/// Research: entity ids below `max` that have a component of type `type_hash`, with its
/// first bytes.
pub fn scan_component(type_hash: u32, max: u32) -> Vec<(u32, Vec<u8>)> {
    world_ctx::with(|ctx| unsafe {
        let game = events::game();
        (1..max)
            .filter_map(|entity| {
                let record = game.entity_record(ctx, entity)?;
                let (addr, size) = components::of(ctx, &record, type_hash)?;
                Some((entity, std::slice::from_raw_parts(addr, (size as usize).min(32)).to_vec()))
            })
            .collect()
    })
    .unwrap_or_default()
}

/// Overwrites an entity's component of type `type_hash`; `data` must be its exact size.
pub fn write_component(entity: u32, type_hash: u32, data: &[u8]) -> bool {
    world_ctx::with(|ctx| unsafe {
        let record = events::game().entity_record(ctx, entity)?;
        let (addr, size) = components::of(ctx, &record, type_hash)?;
        (size as usize == data.len()).then(|| std::ptr::copy_nonoverlapping(data.as_ptr(), addr, data.len()))
    })
    .flatten()
    .is_some()
}

/// Entities with an inventory within `radius` metres of `entity` (loaded ones), players
/// excluded.
pub fn inventories_near(entity: u32, radius: f64) -> Vec<u32> {
    let Some(around) = position(entity) else { return Vec::new() };
    inventories_near_position(around, radius).into_iter().map(|(e, _, _)| e).collect()
}

fn inventories_near_position(around: [f64; 3], radius: f64) -> Vec<(u32, [f64; 3], Option<String>)> {
    inventories_around(around, radius)
}

/// The slots of any entity's inventory: (slot id, item, count).
pub fn inventory(entity: u32) -> Vec<(u64, u32, u32)> {
    world_ctx::with(|ctx| unsafe {
        let game = events::game();
        let Some(setup) = game.inventory_setup(ctx, entity) else { return Vec::new() };
        let mut out = Vec::new();
        for page in game.pages(ctx, setup).pages().iter().filter(|p| p.entity != 0) {
            for slot in 0..crate::game::SLOTS_PER_PAGE {
                let stack = &*page.slots.add(slot as usize);
                out.push((crate::game::slot_id(page.entity, slot), stack.item_id, stack.count));
            }
        }
        out
    })
    .unwrap_or_default()
}

/// Moves a stack between two inventories, outside any player action. A player's
/// equipment, item in hand and action bar are never taken, and never filled.
pub fn move_stack(from: u32, slot: u64, to: u32) -> u32 {
    world_ctx::with(|ctx| unsafe {
        let game = events::game();
        let mut protected = std::collections::HashSet::new();
        if game.is_player(ctx, from) {
            let reserved = game.player_reserved(ctx, from);
            protected.extend(reserved.slots);
            for page in reserved.pages.into_iter().filter(|&p| p != 0) {
                protected.extend((0..crate::game::SLOTS_PER_PAGE).map(|s| crate::game::slot_id(page, s)));
            }
        }
        crate::game_api::move_stack_in(game, ctx, from, slot, to, &protected)
    })
    .unwrap_or(0)
}

// ---- C functions of HostApi v4 ----

pub unsafe extern "C" fn api_players(out: *mut u32, cap: usize) -> usize {
    let players = players();
    std::ptr::copy_nonoverlapping(players.as_ptr(), out, players.len().min(cap));
    players.len()
}

pub unsafe extern "C" fn api_position(entity: u32, out: *mut f64) -> bool {
    match position(entity) {
        Some(p) => {
            std::ptr::copy_nonoverlapping(p.as_ptr(), out, 3);
            true
        }
        None => false,
    }
}

pub unsafe extern "C" fn api_teleport(entity: u32, x: f64, y: f64, z: f64) -> bool {
    teleport(entity, [x, y, z])
}

pub unsafe extern "C" fn api_read_component(entity: u32, type_hash: u32, out: *mut u8, cap: usize) -> usize {
    read_component(entity, type_hash, std::slice::from_raw_parts_mut(out, cap))
}

pub unsafe extern "C" fn api_write_component(entity: u32, type_hash: u32, data: *const u8, len: usize) -> bool {
    write_component(entity, type_hash, std::slice::from_raw_parts(data, len))
}

pub unsafe extern "C" fn api_inventories_near(entity: u32, radius: f64, out: *mut u32, cap: usize) -> usize {
    let found = inventories_near(entity, radius);
    std::ptr::copy_nonoverlapping(found.as_ptr(), out, found.len().min(cap));
    found.len()
}

pub unsafe extern "C" fn api_inventory(entity: u32, out: *mut shroudlink_sdk::Slot, cap: usize) -> usize {
    let slots = inventory(entity);
    for (i, &(id, item, count)) in slots.iter().take(cap).enumerate() {
        *out.add(i) = shroudlink_sdk::Slot { id, item, count };
    }
    slots.len()
}

pub unsafe extern "C" fn api_move_stack(from: u32, slot: u64, to: u32) -> u32 {
    move_stack(from, slot, to)
}

pub unsafe extern "C" fn api_apply_buff(entity: u32, buff: *const u8, seconds: f64) -> bool {
    let mut guid = [0u8; 16];
    std::ptr::copy_nonoverlapping(buff, guid.as_mut_ptr(), 16);
    let keep = if seconds.is_finite() { seconds.clamp(0.0, 3600.0) } else { 0.0 };
    crate::buffs::apply(crate::buffs::Request { target: entity, buff: guid }, std::time::Duration::from_secs_f64(keep))
}

pub unsafe extern "C" fn api_player_of(client: u64) -> u32 {
    player_of(client).unwrap_or(0)
}
