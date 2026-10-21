//! Server mods talking to client addons over shroudlink.
//!
//! Every message is a JSON object with a `"mod"` field naming its mod; the host
//! routes it to the `ServerMod` with that id. The built-in `shroudlink` mod
//! answers a joining client with the list of mods, so the addon knows what the
//! server offers without being updated for new mods.

use std::ffi::c_void;
use std::sync::Mutex;

use chestsort_common::log;
use serde_json::{json, Value};
use shroudlink_sdk::InventoryEvent;

use crate::link_host::Host;

/// A connected client addon (its session nonce).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ClientId(pub u64);

pub trait ServerMod: Send {
    fn id(&self) -> &'static str;
    /// Short description shown by the client addon.
    fn describe(&self) -> Value {
        json!({ "id": self.id() })
    }
    fn on_join(&mut self, _host: &mut Host, _client: ClientId) {}
    fn on_message(&mut self, host: &mut Host, client: ClientId, message: &Value);
    fn on_leave(&mut self, _host: &mut Host, _client: ClientId) {}
    /// Slots of `event.from` that must not move (`game`: the event for the game API).
    fn keep(&mut self, _game: *mut c_void, _event: &InventoryEvent) -> Vec<u64> {
        Vec::new()
    }
    /// Moves stacks before the game's own transfer; returns the number of items moved.
    fn on_transfer(&mut self, _game: *mut c_void, _event: &InventoryEvent) -> u32 {
        0
    }
    /// Game event type ids the mod wants (asked once, when it is registered).
    fn game_events(&self) -> Vec<u32> {
        Vec::new()
    }
    fn on_game_event(&mut self, _hash: u32, _data: &[u8]) {}
}

static MODS: Mutex<Vec<Box<dyn ServerMod>>> = Mutex::new(Vec::new());

pub fn register(m: Box<dyn ServerMod>) {
    log!("shroudlink: mod '{}' registered", m.id());
    let mut mods = MODS.lock().unwrap_or_else(|e| e.into_inner());
    mods.push(m);
    update_subscriptions(&mods);
}

/// Game event types any mod wants; checked for every queued event, so kept outside the
/// mod lock.
static SUBSCRIBED: Mutex<Vec<u32>> = Mutex::new(Vec::new());

fn update_subscriptions(mods: &[Box<dyn ServerMod>]) {
    let mut all: Vec<u32> = mods.iter().flat_map(|m| m.game_events()).collect();
    all.sort_unstable();
    all.dedup();
    *SUBSCRIBED.lock().unwrap_or_else(|e| e.into_inner()) = all;
}

pub(crate) fn wants_game_event(hash: u32) -> bool {
    SUBSCRIBED.lock().unwrap_or_else(|e| e.into_inner()).binary_search(&hash).is_ok()
}

/// Hands this frame's subscribed game events to the mods that want them.
pub(crate) fn game_events(events: &[(u32, &[u8])]) {
    with_mods(|mods| {
        for m in mods.iter_mut() {
            let wanted = m.game_events();
            for (hash, data) in events.iter().filter(|(h, _)| wanted.contains(h)) {
                m.on_game_event(*hash, data);
            }
        }
    });
}

/// Swaps in a new build of a mod (hot reload) and welcomes the connected clients again.
pub(crate) fn replace(host: &mut Host, mut m: Box<dyn ServerMod>) {
    let clients = host.clients();
    with_mods(|mods| {
        for &client in &clients {
            m.on_join(host, client);
        }
        match mods.iter_mut().find(|old| old.id() == m.id()) {
            Some(old) => *old = m,
            None => mods.push(m),
        }
        update_subscriptions(mods);
    });
}

/// Messages mods sent during an inventory event, delivered by the next network update.
static QUEUED: Mutex<Vec<(ClientId, Value)>> = Mutex::new(Vec::new());

pub(crate) fn queue(client: ClientId, message: Value) {
    QUEUED.lock().unwrap_or_else(|e| e.into_inner()).push((client, message));
}

pub(crate) fn take_queued() -> Vec<(ClientId, Value)> {
    std::mem::take(&mut *QUEUED.lock().unwrap_or_else(|e| e.into_inner()))
}

/// Every mod's slots to keep in an inventory event.
pub(crate) fn keep(game: *mut c_void, event: &InventoryEvent) -> Vec<u64> {
    with_mods(|mods| mods.iter_mut().flat_map(|m| m.keep(game, event)).collect())
}

/// Lets every mod move stacks; returns the items moved in total.
pub(crate) fn transfer(game: *mut c_void, event: &InventoryEvent) -> u32 {
    with_mods(|mods| mods.iter_mut().map(|m| m.on_transfer(game, event)).sum())
}

fn with_mods<R>(f: impl FnOnce(&mut Vec<Box<dyn ServerMod>>) -> R) -> R {
    f(&mut MODS.lock().unwrap_or_else(|e| e.into_inner()))
}

/// Identifies this server to addons, which keep mod data per server: the server name
/// from `enshrouded_server.json`.
fn server_id() -> &'static str {
    static ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    ID.get_or_init(|| {
        std::fs::read_to_string(chestsort_common::game_dir().join("enshrouded_server.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v.get("name")?.as_str().map(str::to_owned))
            .unwrap_or_else(|| "server".into())
    })
}

/// Ids of the registered mods.
pub(crate) fn ids() -> Vec<String> {
    with_mods(|mods| mods.iter().map(|m| m.id().to_owned()).collect())
}

pub(crate) fn join(host: &mut Host, client: ClientId) {
    with_mods(|mods| {
        let list: Vec<Value> = mods.iter().map(|m| m.describe()).collect();
        host.send(client, &json!({ "mod": "shroudlink", "op": "welcome", "mods": list, "server": server_id(), "features": crate::hooks::working() }));
        for m in mods.iter_mut() {
            m.on_join(host, client);
        }
    });
}

pub(crate) fn leave(host: &mut Host, client: ClientId) {
    crate::world::forget_client(client.0);
    with_mods(|mods| mods.iter_mut().for_each(|m| m.on_leave(host, client)));
}

pub(crate) fn message(host: &mut Host, client: ClientId, message: &Value) {
    if crate::game_events::probing() {
        log!("probe: message {}", message.to_string().chars().take(160).collect::<String>());
    }
    let Some(target) = message.get("mod").and_then(Value::as_str) else {
        log!("shroudlink: message without \"mod\" from {:016x}", client.0);
        return;
    };
    if target == "shroudlink" {
        match message.get("op").and_then(Value::as_str) {
            Some("ping") => host.send(client, &json!({ "mod": "shroudlink", "op": "pong", "echo": message.get("echo") })),
            // a reloaded addon continues its session and wants the welcome again
            Some("hello_again") => join(host, client),
            Some("identify") => {
                let pages: Vec<u32> = message.get("pages").and_then(Value::as_array).into_iter().flatten().filter_map(|v| v.as_u64().map(|p| p as u32)).collect();
                crate::world::identify(client.0, &pages);
            }
            // test automation (only with a shroudlink_probe file): players, positions, teleport
            Some("debug_players") if crate::game_events::probing() => {
                let players: Vec<Value> = crate::world::players().into_iter().map(|p| json!({ "entity": p, "position": crate::world::position(p) })).collect();
                log!("debug_identity: {}", json!({ "client": format!("{:016x}", client.0), "player": crate::world::player_of(client.0) }));
                log!("debug_players: {}", Value::Array(players.clone()));
                host.send(client, &json!({ "mod": "shroudlink", "op": "debug_players", "players": players }));
            }
            Some("debug_inventories") if crate::game_events::probing() => {
                let radius = message.get("radius").and_then(Value::as_f64).unwrap_or(30.0);
                let player = crate::world::players().first().copied();
                let near = player.and_then(crate::world::position).map(|p| crate::world::inventories_near(p, radius)).unwrap_or_default();
                let list: Vec<Value> = near.iter().map(|(e, p, n)| json!({ "entity": e, "position": p, "name": n })).collect();
                log!("debug_inventories: {}", Value::Array(list));
            }
            Some("debug_face") if crate::game_events::probing() => {
                let yaw = message.get("yaw").and_then(Value::as_f64).unwrap_or(0.0);
                if let Some(p) = crate::world::players().first().copied() {
                    log!("debug_face: {}", crate::world::face(p, yaw));
                }
            }
            Some("debug_teleport") if crate::game_events::probing() => {
                let num = |k: &str| message.get(k).and_then(Value::as_f64);
                let player = message.get("player").and_then(Value::as_u64).map(|p| p as u32).or_else(|| crate::world::players().first().copied());
                let done = player.and_then(|p| {
                    let here = crate::world::position(p)?;
                    let to = match (num("x"), num("y"), num("z")) {
                        (Some(x), Some(y), Some(z)) => [x, y, z],
                        _ => [here[0] + num("dx").unwrap_or(0.0), here[1] + num("dy").unwrap_or(0.0), here[2] + num("dz").unwrap_or(0.0)],
                    };
                    Some(crate::world::teleport(p, to))
                });
                host.send(client, &json!({ "mod": "shroudlink", "op": "debug_teleport", "done": done }));
            }
            _ => {}
        }
        return;
    }
    with_mods(|mods| match mods.iter_mut().find(|m| m.id() == target) {
        Some(m) => m.on_message(host, client, message),
        None => log!("shroudlink: message for unknown mod '{target}'"),
    });
}
