//! Server end of shroudlink: hooks the custom player string replication of the
//! game (see `shroudlink` crate docs) and routes JSON messages to server mods.
//!
//! - `set_string` (`FUN_140909660(out, strings, oldId, &text)`) stores a rename
//!   sent by a client. Packets are consumed here and never reach the table.
//! - `next_pending` (`FUN_1409234b0(strings, &id, &text, &size, slot, gen)`) is
//!   polled per connected player slot by the server's send loop; queued packets
//!   for that slot are handed out first.
//! - `mark_sent` (`FUN_140942620(strings, id, &text, slot, size)`) confirms a sent
//!   string; for our packets it pops the queue instead of touching the table.
//!
//! All three run inside the server's network update on the game thread.

use std::collections::{HashMap, VecDeque};
use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use chestsort_common::log;
use chestsort_common::scan::Module;
use retour::GenericDetour;
use serde_json::Value;
use shroudlink::{decode_packet, is_packet, split_message, string_id, Frame, Reassembler, PROTOCOL_VERSION};

use crate::mods::{self, ClientId};

const SIG_SET_STRING: &str = "48 89 5C 24 10 55 56 57 41 54 41 57";
const SIG_NEXT_PENDING: &str = "48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 57 41 54 41 55 41 56 41 57 48 83 EC 20 8B 7C 24 70";
const SIG_MARK_SENT: &str = "48 89 74 24 20 57 48 83 EC 20 83 B9 E4 01 00 00 00";

/// Beacon repeat interval for slots without a session. The beacon text is the same each
/// time (same string id), so a vanilla client only ever stores it once.
const BEACON_INTERVAL: Duration = Duration::from_secs(30);

/// Player slots of the server (`FUN_1407afb50` iterates 0x11 peers).
pub const SLOTS: usize = 17;

#[repr(C)]
pub struct StrView {
    ptr: *const u8,
    len: usize,
}

type SetStringFn = unsafe extern "system" fn(out: *mut u8, strings: *mut c_void, old: u64, text: *const StrView) -> *mut u8;
type NextPendingFn =
    unsafe extern "system" fn(strings: *mut c_void, id: *mut u64, text: *mut StrView, size: *mut u32, slot: u32, gen: i32) -> u64;
type MarkSentFn = unsafe extern "system" fn(strings: *mut c_void, id: u64, text: *const StrView, slot: u32, size: i32);

struct Hooks {
    set_string: GenericDetour<SetStringFn>,
    next_pending: GenericDetour<NextPendingFn>,
    mark_sent: GenericDetour<MarkSentFn>,
}

// the detours are only read after installation
unsafe impl Send for Hooks {}
unsafe impl Sync for Hooks {}

static HOOKS: OnceLock<Hooks> = OnceLock::new();
static STATE: Mutex<Option<Host>> = Mutex::new(None);

pub struct Packet {
    pub id: u64,
    pub text: String,
}

#[derive(Default)]
struct SlotState {
    /// connection generation as passed by the send loop; changes on reconnect
    gen: Option<i32>,
    /// last beacon; repeated while no addon claimed the slot (late install, hot reload)
    beacon_at: Option<Instant>,
    outbox: VecDeque<Packet>,
    session: Option<u64>,
}

#[derive(Default)]
pub struct Host {
    slots: Vec<SlotState>,
    sessions: HashMap<u64, u8>,
    reassembler: Reassembler,
    next_msg: u32,
}

impl Host {
    fn slot(&mut self, slot: usize) -> &mut SlotState {
        if self.slots.len() < SLOTS {
            self.slots.resize_with(SLOTS, SlotState::default);
        }
        &mut self.slots[slot]
    }

    fn queue(&mut self, slot: usize, text: String) {
        let id = string_id(text.as_bytes());
        self.slot(slot).outbox.push_back(Packet { id, text });
    }

    /// Sends a JSON message to a connected addon.
    pub fn send(&mut self, client: ClientId, message: &Value) {
        let Some(&slot) = self.sessions.get(&client.0) else { return };
        let bytes = serde_json::to_vec(message).unwrap_or_default();
        let msg = self.next_msg;
        self.next_msg = self.next_msg.wrapping_add(1);
        for packet in split_message(client.0, msg, &bytes) {
            self.queue(slot as usize, packet);
        }
    }

    pub fn clients(&self) -> Vec<ClientId> {
        self.sessions.keys().map(|&n| ClientId(n)).collect()
    }

    fn end_session(&mut self, slot: usize) {
        if let Some(nonce) = self.slot(slot).session.take() {
            self.sessions.remove(&nonce);
            self.reassembler.forget(nonce);
            log!("shroudlink: client {nonce:016x} on slot {slot} left");
            mods::leave(self, ClientId(nonce));
        }
    }

    /// A rename text that is a shroudlink packet (checked by the caller).
    pub fn receive_packet(&mut self, bytes: &[u8]) {
        if let Some(frame) = decode_packet(bytes).as_deref().and_then(Frame::decode) {
            self.receive(frame);
        }
    }

    /// The send loop asks for slot `slot` (connection generation `gen`): the packet to
    /// send next, if any. Tracks reconnects and repeats the beacon while unclaimed.
    pub fn poll(&mut self, slot: usize, gen: i32) -> Option<&Packet> {
        if slot >= SLOTS {
            return None;
        }
        // mods: messages sent during inventory events, and new builds of their DLLs
        for (client, message) in mods::take_queued() {
            self.send(client, &message);
        }
        crate::plugins::reload_changed(self);
        if self.slot(slot).gen != Some(gen) {
            self.end_session(slot);
            let state = self.slot(slot);
            state.outbox.clear();
            state.gen = Some(gen);
            state.beacon_at = None;
        }
        let state = self.slot(slot);
        if state.session.is_none() && state.beacon_at.is_none_or(|t| t.elapsed() >= BEACON_INTERVAL) {
            // beacon: tells an addon on this connection that a host is here and which slot it is
            state.beacon_at = Some(Instant::now());
            let beacon = shroudlink::encode_packet(&Frame::WhoAmI { nonce: 0, slot: slot as u8 }.encode());
            self.queue(slot, beacon);
        }
        self.slots[slot].outbox.front()
    }

    /// The game confirms it sent string `id` from `text`; true when it was our packet.
    pub fn confirm(&mut self, slot: usize, id: u64, text: *const u8) -> bool {
        if slot < SLOTS && self.slot(slot).outbox.front().is_some_and(|p| p.id == id && p.text.as_ptr() == text) {
            self.slot(slot).outbox.pop_front();
            true
        } else {
            false
        }
    }

    /// Test helper: drains the beacon of `slot` and binds session `nonce` to it.
    #[cfg(test)]
    pub fn connect_for_test(&mut self, slot: usize, nonce: u64) {
        while let Some(p) = self.poll(slot, 1) {
            let (id, ptr) = (p.id, p.text.as_ptr());
            self.confirm(slot, id, ptr);
        }
        let claim = shroudlink::encode_packet(&Frame::Claim { nonce, slot: slot as u8 }.encode());
        self.receive_packet(claim.as_bytes());
    }

    /// Test helper: the JSON messages queued for session `nonce` on `slot`.
    #[cfg(test)]
    pub fn received_for_test(&mut self, slot: usize, nonce: u64) -> Vec<Value> {
        let mut r = Reassembler::default();
        let mut out = Vec::new();
        while let Some(p) = self.poll(slot, 1) {
            let (id, ptr) = (p.id, p.text.as_ptr());
            if let Some(Frame::Data { nonce: n, msg, chunk, count, bytes }) = decode_packet(p.text.as_bytes()).as_deref().and_then(Frame::decode) {
                if n == nonce {
                    if let Some(m) = r.push(n, msg, chunk, count, bytes) {
                        out.push(serde_json::from_slice(&m).unwrap());
                    }
                }
            }
            self.confirm(slot, id, ptr);
        }
        out
    }

    fn receive(&mut self, frame: Frame) {
        match frame {
            Frame::Hello { nonce, version } => {
                if version != PROTOCOL_VERSION {
                    log!("shroudlink: client {nonce:016x} speaks protocol {version}, host {PROTOCOL_VERSION}; ignored");
                    return;
                }
                // the rename message does not say which peer sent it, so ask every connected slot
                let connected: Vec<usize> = (0..SLOTS).filter(|&s| self.slot(s).gen.is_some()).collect();
                for slot in connected {
                    let text = shroudlink::encode_packet(&Frame::WhoAmI { nonce, slot: slot as u8 }.encode());
                    self.queue(slot, text);
                }
            }
            Frame::Claim { nonce, slot } => {
                let slot = slot as usize;
                if slot >= SLOTS || self.slot(slot).gen.is_none() {
                    return;
                }
                self.end_session(slot);
                self.slot(slot).session = Some(nonce);
                self.sessions.insert(nonce, slot as u8);
                log!("shroudlink: client {nonce:016x} joined on slot {slot}");
                mods::join(self, ClientId(nonce));
            }
            Frame::Data { nonce, msg, chunk, count, bytes } => {
                if !self.sessions.contains_key(&nonce) {
                    return;
                }
                if let Some(message) = self.reassembler.push(nonce, msg, chunk, count, bytes) {
                    match serde_json::from_slice::<Value>(&message) {
                        Ok(value) => mods::message(self, ClientId(nonce), &value),
                        Err(e) => log!("shroudlink: bad message from {nonce:016x}: {e}"),
                    }
                }
            }
            Frame::WhoAmI { .. } => {}
        }
    }
}

fn with_host<R>(f: impl FnOnce(&mut Host) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(Host::default))
}

unsafe extern "system" fn set_string_hook(out: *mut u8, strings: *mut c_void, old: u64, text: *const StrView) -> *mut u8 {
    let bytes = std::slice::from_raw_parts((*text).ptr, (*text).len);
    if !is_packet(bytes) {
        return HOOKS.get().unwrap().set_string.call(out, strings, old, text);
    }
    with_host(|host| host.receive_packet(bytes));
    // report an error: the rename handler then leaves the entity untouched
    *out = 0x1f;
    *(out.add(8) as *mut u64) = 0;
    out
}

unsafe extern "system" fn next_pending_hook(
    strings: *mut c_void,
    id: *mut u64,
    text: *mut StrView,
    size: *mut u32,
    slot: u32,
    gen: i32,
) -> u64 {
    let ours = with_host(|host| match host.poll(slot as usize, gen) {
        Some(packet) => {
            *id = packet.id;
            *text = StrView { ptr: packet.text.as_ptr(), len: packet.text.len() };
            *size = (packet.text.len() as u32 + 10 + 3) & !3;
            true
        }
        None => false,
    });
    if ours {
        return 1;
    }
    HOOKS.get().unwrap().next_pending.call(strings, id, text, size, slot, gen)
}

unsafe extern "system" fn mark_sent_hook(strings: *mut c_void, id: u64, text: *const StrView, slot: u32, size: i32) {
    let ours = with_host(|host| host.confirm(slot as usize, id, (*text).ptr));
    if !ours {
        HOOKS.get().unwrap().mark_sent.call(strings, id, text, slot, size);
    }
}

pub fn install(module: &Module) -> Result<(), String> {
    let set_string = module.find(SIG_SET_STRING)?;
    let next_pending = module.find(SIG_NEXT_PENDING)?;
    let mark_sent = module.find(SIG_MARK_SENT)?;
    unsafe {
        let hooks = Hooks {
            set_string: GenericDetour::new(std::mem::transmute::<usize, SetStringFn>(set_string), set_string_hook)
                .map_err(|e| format!("set_string hook: {e}"))?,
            next_pending: GenericDetour::new(std::mem::transmute::<usize, NextPendingFn>(next_pending), next_pending_hook)
                .map_err(|e| format!("next_pending hook: {e}"))?,
            mark_sent: GenericDetour::new(std::mem::transmute::<usize, MarkSentFn>(mark_sent), mark_sent_hook)
                .map_err(|e| format!("mark_sent hook: {e}"))?,
        };
        let hooks = HOOKS.get_or_init(|| hooks);
        hooks.set_string.enable().map_err(|e| e.to_string())?;
        hooks.next_pending.enable().map_err(|e| e.to_string())?;
        hooks.mark_sent.enable().map_err(|e| e.to_string())?;
    }
    log!(
        "shroudlink host ready (protocol {PROTOCOL_VERSION}): set +{:#x}, next +{:#x}, sent +{:#x}",
        module.rva(set_string),
        module.rva(next_pending),
        module.rva(mark_sent)
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::{self, ServerMod};
    use shroudlink::{encode_packet, split_message};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static ECHOED: AtomicUsize = AtomicUsize::new(0);

    struct Echo;
    impl ServerMod for Echo {
        fn id(&self) -> &'static str {
            "echo"
        }
        fn on_message(&mut self, host: &mut Host, client: ClientId, message: &Value) {
            ECHOED.fetch_add(1, Ordering::SeqCst);
            host.send(client, &serde_json::json!({ "mod": "echo", "back": message["text"] }));
        }
    }

    /// Everything the host sends to `slot` until its outbox is empty, as decoded frames.
    fn drain(host: &mut Host, slot: usize, gen: i32) -> Vec<Frame> {
        let mut frames = Vec::new();
        while let Some(p) = host.poll(slot, gen) {
            let (id, ptr) = (p.id, p.text.as_ptr());
            frames.push(Frame::decode(&decode_packet(p.text.as_bytes()).unwrap()).unwrap());
            assert!(host.confirm(slot, id, ptr));
            if frames.len() > 1000 {
                panic!("outbox does not drain");
            }
        }
        frames
    }

    /// Reassembles the JSON messages contained in `frames` for session `nonce`.
    fn messages(frames: &[Frame], nonce: u64) -> Vec<Value> {
        let mut r = Reassembler::default();
        frames
            .iter()
            .filter_map(|f| match f {
                Frame::Data { nonce: n, msg, chunk, count, bytes } if *n == nonce => r.push(*n, *msg, *chunk, *count, bytes.clone()),
                _ => None,
            })
            .map(|m| serde_json::from_slice(&m).unwrap())
            .collect()
    }

    #[test]
    fn full_session() {
        mods::register(Box::new(Echo));
        let mut host = Host::default();

        // a new connection on slot 3 gets the beacon, other slots nothing
        let frames = drain(&mut host, 3, 7);
        assert_eq!(frames, [Frame::WhoAmI { nonce: 0, slot: 3 }]);
        assert!(drain(&mut host, 3, 7).is_empty(), "beacon repeats only after the interval");

        // the addon claims the slot and receives the mod list
        let nonce = 0x1234;
        host.receive_packet(encode_packet(&Frame::Claim { nonce, slot: 3 }.encode()).as_bytes());
        let welcome = messages(&drain(&mut host, 3, 7), nonce);
        assert_eq!(welcome[0]["mod"], "shroudlink");
        assert_eq!(welcome[0]["op"], "welcome");
        assert!(welcome[0]["mods"].as_array().unwrap().iter().any(|m| m["id"] == "echo"));

        // a large chunked message reaches the mod and its answer comes back
        let text = "x".repeat(1000);
        let msg = serde_json::to_vec(&serde_json::json!({ "mod": "echo", "text": text })).unwrap();
        for packet in split_message(nonce, 1, &msg) {
            host.receive_packet(packet.as_bytes());
        }
        assert_eq!(ECHOED.load(Ordering::SeqCst), 1);
        let back = messages(&drain(&mut host, 3, 7), nonce);
        assert_eq!(back[0]["back"].as_str().unwrap().len(), 1000);

        // data from unknown sessions is ignored
        for packet in split_message(0xdead, 2, &msg) {
            host.receive_packet(packet.as_bytes());
        }
        assert_eq!(ECHOED.load(Ordering::SeqCst), 1);

        // reconnect (new generation): the session ends and a new beacon goes out
        let frames = drain(&mut host, 3, 8);
        assert_eq!(frames, [Frame::WhoAmI { nonce: 0, slot: 3 }]);
        assert!(host.clients().is_empty());
    }

    #[test]
    fn foreign_strings_are_not_ours() {
        let mut host = Host::default();
        let other = String::from("a chest name");
        assert!(!host.confirm(2, string_id(other.as_bytes()), other.as_ptr()));
        assert!(host.poll(SLOTS, 1).is_none());
    }
}
