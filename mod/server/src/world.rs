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
pub fn inventories_near(around: [f64; 3], radius: f64) -> Vec<(u32, [f64; 3], Option<String>)> {
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

pub unsafe extern "C" fn api_player_of(client: u64) -> u32 {
    player_of(client).unwrap_or(0)
}
