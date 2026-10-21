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
}

static MODS: Mutex<Vec<Box<dyn ServerMod>>> = Mutex::new(Vec::new());

pub fn register(m: Box<dyn ServerMod>) {
    log!("shroudlink: mod '{}' registered", m.id());
    MODS.lock().unwrap_or_else(|e| e.into_inner()).push(m);
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

pub(crate) fn join(host: &mut Host, client: ClientId) {
    with_mods(|mods| {
        let list: Vec<Value> = mods.iter().map(|m| m.describe()).collect();
        host.send(client, &json!({ "mod": "shroudlink", "op": "welcome", "mods": list }));
        for m in mods.iter_mut() {
            m.on_join(host, client);
        }
    });
}

pub(crate) fn leave(host: &mut Host, client: ClientId) {
    with_mods(|mods| mods.iter_mut().for_each(|m| m.on_leave(host, client)));
}

pub(crate) fn message(host: &mut Host, client: ClientId, message: &Value) {
    let Some(target) = message.get("mod").and_then(Value::as_str) else {
        log!("shroudlink: message without \"mod\" from {:016x}", client.0);
        return;
    };
    if target == "shroudlink" {
        match message.get("op").and_then(Value::as_str) {
            Some("ping") => host.send(client, &json!({ "mod": "shroudlink", "op": "pong", "echo": message.get("echo") })),
            // a reloaded addon continues its session and wants the welcome again
            Some("hello_again") => join(host, client),
            _ => {}
        }
        return;
    }
    with_mods(|mods| match mods.iter_mut().find(|m| m.id() == target) {
        Some(m) => m.on_message(host, client, message),
        None => log!("shroudlink: message for unknown mod '{target}'"),
    });
}
