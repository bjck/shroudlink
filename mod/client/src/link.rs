//! Client end of shroudlink (see the `shroudlink` crate for the protocol).
//!
//! - `client_tick` (`FUN_7ff622e73970(client)`, a per-frame client system that
//!   occasionally renames the local player) gives us the client object and a
//!   game-thread tick in which queued packets are sent.
//! - `send_rename` (`FUN_7ff622e68b40(client, out, playerIndex, &text)`) sends a
//!   `CustomPlayerStringClientMessage`; we call it with our packets.
//! - `store_string` (`FUN_7ff623464bf0(out, strings, id, &text, flags)`) stores a
//!   string received from the server; packets are consumed there instead.
//!
//! The addon never sends anything before the server's beacon (`WhoAmI` with
//! nonce 0) arrived, so vanilla servers never see a packet.

use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use chestsort_common::log;
use chestsort_common::scan::{call_targets, Module};
use retour::GenericDetour;
use serde_json::{json, Value};
use shroudlink::{decode_packet, encode_packet, is_packet, split_message, Frame, Reassembler};

/// Any signature of the game, used to wait for the DRM to decrypt the code.
pub const PROBE_SIGNATURE: &str = SIG_CLIENT_TICK;

/// `FUN_7ff622e73970(client)`: renames the local player when needed; read for offsets only.
const SIG_CLIENT_TICK: &str = "48 89 5C 24 10 57 48 81 EC 90 01 00 00";
/// `FUN_7ff622e7e7b0(client, a, flag)`: the client's per-frame network update; our tick.
const SIG_CLIENT_UPDATE: &str = "44 88 44 24 18 48 89 54 24 10 55 53 56 57";
const SIG_SEND_RENAME: &str = "48 89 5C 24 08 48 89 74 24 10 48 89 7C 24 18 4C 89 74 24 20 55 48 8D AC 24 50 FF FF FF";
const SIG_STORE_STRING: &str = "48 89 5C 24 08 4C 89 44 24 18 55 56 57 41 54 41 55 41 56 41 57 48 83 EC 20 33 ED";

/// `slot:nonce` of the current session, so a hot-reloaded copy continues it.
const SESSION_VAR: &str = "SHROUDLINK_CLIENT_SESSION";

/// Packets sent per client tick, keeps bursts (large UI messages) from flooding the connection.
const PACKETS_PER_TICK: usize = 24;

#[repr(C)]
struct StrView {
    ptr: *const u8,
    len: usize,
}

type ClientUpdateFn = unsafe extern "system" fn(client: *mut u8, a: *mut c_void, flag: u8);
type SendRenameFn = unsafe extern "system" fn(client: *mut u8, out: *mut u8, player: u32, text: *const StrView) -> *mut u8;
type StoreStringFn = unsafe extern "system" fn(out: *mut u8, strings: *mut c_void, id: u64, text: *const StrView, flags: u32) -> *mut u8;
type PlayerIndexFn = unsafe extern "system" fn(handle: *const u32) -> u32;

struct Game {
    tick: GenericDetour<ClientUpdateFn>,
    store: GenericDetour<StoreStringFn>,
    send_rename: SendRenameFn,
    player_index: PlayerIndexFn,
    /// offset of the local player handle in the client object (`cmp [rcx+X], 0` in the tick)
    player_offset: usize,
}

unsafe impl Send for Game {}
unsafe impl Sync for Game {}

static GAME: OnceLock<Game> = OnceLock::new();
static STATE: Mutex<Option<Link>> = Mutex::new(None);

/// Handlers for messages from server mods, by `"mod"` id (the UI registers itself as "ui").
pub type Handler = fn(&Value);
static HANDLERS: Mutex<Vec<(&'static str, Handler)>> = Mutex::new(Vec::new());

pub fn on_message(mod_id: &'static str, handler: Handler) {
    HANDLERS.lock().unwrap().push((mod_id, handler));
}

#[derive(Default)]
struct Link {
    /// our session id, chosen when the server's beacon arrives
    nonce: u64,
    slot: Option<u8>,
    outbox: VecDeque<String>,
    reassembler: Reassembler,
    next_msg: u32,
    server_mods: Vec<Value>,
}

impl Link {
    fn send(&mut self, message: &Value) {
        if self.slot.is_none() {
            return;
        }
        let bytes = serde_json::to_vec(message).unwrap_or_default();
        let msg = self.next_msg;
        self.next_msg = self.next_msg.wrapping_add(1);
        self.outbox.extend(split_message(self.nonce, msg, &bytes));
    }
}

fn with_link<R>(f: impl FnOnce(&mut Link) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(Link::default))
}

/// Sends a JSON message to the server mods (dropped when not connected to a shroudlink host).
pub fn send(message: Value) {
    with_link(|link| link.send(&message));
}

pub fn connected() -> bool {
    with_link(|link| link.slot.is_some())
}

pub fn server_mods() -> Vec<Value> {
    with_link(|link| link.server_mods.clone())
}

fn new_nonce() -> u64 {
    let t = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
    shroudlink::string_id(&t.to_le_bytes()) | 1
}

fn receive(frame: Frame) -> Option<Value> {
    match frame {
        Frame::WhoAmI { nonce, slot } => {
            with_link(|link| {
                // beacon (nonce 0): a shroudlink host greets a new connection
                if nonce == 0 || nonce == link.nonce {
                    *link = Link { nonce: new_nonce(), slot: Some(slot), ..Link::default() };
                    let claim = encode_packet(&Frame::Claim { nonce: link.nonce, slot }.encode());
                    link.outbox.push_back(claim);
                    std::env::set_var(SESSION_VAR, format!("{slot}:{:x}", link.nonce));
                    log!("shroudlink: host found, we are slot {slot}");
                }
            });
            None
        }
        Frame::Data { nonce, msg, chunk, count, bytes } => {
            let message = with_link(|link| {
                if nonce != link.nonce {
                    return None;
                }
                link.reassembler.push(nonce, msg, chunk, count, bytes)
            })?;
            match serde_json::from_slice::<Value>(&message) {
                Ok(v) => Some(v),
                Err(e) => {
                    log!("shroudlink: bad message from host: {e}");
                    None
                }
            }
        }
        Frame::Hello { .. } | Frame::Claim { .. } => None,
    }
}

pub(crate) fn dispatch(message: &Value) {
    let target = message.get("mod").and_then(Value::as_str).unwrap_or("");
    if target == "shroudlink" {
        match message.get("op").and_then(Value::as_str) {
            Some("welcome") => {
                let mods = message.get("mods").and_then(Value::as_array).cloned().unwrap_or_default();
                log!("shroudlink: connected, server mods: {}", Value::Array(mods.clone()));
                with_link(|link| link.server_mods = mods.clone());
                // the addon's version first: the server warns about an older addon by its absence
                let ours = shroudlink::ui::UI_PROTOCOL;
                send(json!({ "mod": "shroudlink", "op": "hello", "version": env!("CARGO_PKG_VERSION"), "ui_protocol": ours }));
                let theirs = message.get("ui_protocol").and_then(Value::as_u64).unwrap_or(1) as u32;
                if theirs < ours {
                    log!("shroudlink: server speaks UI protocol {theirs}, addon {ours}");
                    crate::ui::note("This server runs an older shroudlink: some mod windows may look wrong. Ask the server owner to update it.".into());
                }
                send(json!({ "mod": "shroudlink", "op": "ping", "echo": "hello from the client addon" }));
                // mods that show players use it from the start, not only after the first chest
                if let Some(name) = crate::steam::persona_name() {
                    send(json!({ "mod": "shroudlink", "op": "name", "name": name }));
                }
                crate::slots::on_welcome(message.get("server").and_then(Value::as_str).unwrap_or("server"), &mods);
            }
            Some("pong") => log!("shroudlink: pong {}", message.get("echo").unwrap_or(&Value::Null)),
            Some("layout") => {
                let pages: Vec<u32> = message.get("pages").and_then(Value::as_array).into_iter().flatten().filter_map(|v| v.as_u64().map(|p| p as u32)).collect();
                crate::slots::server_layout(pages);
            }
            _ => {}
        }
    }
    let handlers: Vec<Handler> = HANDLERS.lock().unwrap().iter().filter(|(m, _)| *m == target).map(|(_, h)| *h).collect();
    for h in handlers {
        h(message);
    }
}

unsafe extern "system" fn store_hook(out: *mut u8, strings: *mut c_void, id: u64, text: *const StrView, flags: u32) -> *mut u8 {
    let bytes = std::slice::from_raw_parts((*text).ptr, (*text).len);
    if !is_packet(bytes) {
        return GAME.get().unwrap().store.call(out, strings, id, text, flags);
    }
    if let Some(message) = decode_packet(bytes).as_deref().and_then(Frame::decode).and_then(receive) {
        dispatch(&message);
    }
    *out = 0; // success, the receive handler moves on
    out
}

static TICKS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// When the client last ticked (it only ticks while in a world).
static LAST_TICK: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

/// Milliseconds since the last client tick (None: never ticked).
pub fn since_last_tick() -> Option<u128> {
    LAST_TICK.lock().unwrap().map(|t| t.elapsed().as_millis())
}

unsafe extern "system" fn tick_hook(client: *mut u8, a: *mut c_void, flag: u8) {
    *LAST_TICK.lock().unwrap() = Some(std::time::Instant::now());
    let game = GAME.get().unwrap();
    let n = TICKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if n == 0 || n.is_power_of_two() && n >= 1024 {
        log!("shroudlink: client tick #{n}");
    }
    let packets: Vec<String> = with_link(|link| {
        let n = link.outbox.len().min(PACKETS_PER_TICK);
        link.outbox.drain(..n).collect()
    });
    if !packets.is_empty() {
        let player = (game.player_index)(client.add(game.player_offset) as *const u32);
        for packet in &packets {
            let mut out = [0u8; 16];
            let view = StrView { ptr: packet.as_ptr(), len: packet.len() };
            let result = (game.send_rename)(client, out.as_mut_ptr(), player, &view);
            if *result != 0 {
                log!("shroudlink: sending failed (error {})", *result);
            } else {
                log!("shroudlink: sent {} byte packet as player {player}", packet.len());
            }
        }
    }
    crate::ui::tick();
    game.tick.call(client, a, flag);
}

/// Continues the session of a replaced copy (development hot reload).
fn restore_session() {
    let Ok(v) = std::env::var(SESSION_VAR) else { return };
    let Some((slot, nonce)) = v.split_once(':') else { return };
    if let (Ok(slot), Ok(nonce)) = (slot.parse::<u8>(), u64::from_str_radix(nonce, 16)) {
        with_link(|link| {
            link.slot = Some(slot);
            link.nonce = nonce;
        });
        log!("shroudlink: continuing session on slot {slot}");
        // ask the host for the mod list again
        send(json!({ "mod": "shroudlink", "op": "hello_again" }));
    }
}

pub fn uninstall() {
    if let Some(game) = GAME.get() {
        unsafe {
            let _ = game.tick.disable();
            let _ = game.store.disable();
        }
    }
}

pub fn install(module: &Module) -> Result<(), String> {
    let tick = module.find(SIG_CLIENT_TICK)?;
    let send_rename = module.find(SIG_SEND_RENAME)?;
    let store = module.find(SIG_STORE_STRING)?;
    unsafe {
        // cmp dword ptr [rcx + disp32], 0  right after the prologue
        let cmp = (tick + 0x0d) as *const u8;
        if *cmp != 0x83 || *cmp.add(1) != 0xb9 {
            return Err("client tick layout changed".into());
        }
        let player_offset = (cmp.add(2) as *const u32).read_unaligned() as usize;
        let player_index = *call_targets(tick, 0x30).first().ok_or("client tick has no calls")?;
        let game = Game {
            tick: GenericDetour::new(std::mem::transmute::<usize, ClientUpdateFn>(module.find(SIG_CLIENT_UPDATE)?), tick_hook)
                .map_err(|e| e.to_string())?,
            store: GenericDetour::new(std::mem::transmute::<usize, StoreStringFn>(store), store_hook).map_err(|e| e.to_string())?,
            send_rename: std::mem::transmute::<usize, SendRenameFn>(send_rename),
            player_index: std::mem::transmute::<usize, PlayerIndexFn>(player_index),
            player_offset,
        };
        let game = GAME.get_or_init(|| game);
        restore_session();
        game.store.enable().map_err(|e| e.to_string())?;
        game.tick.enable().map_err(|e| e.to_string())?;
        log!(
            "shroudlink client hooks: tick +{:#x}, send +{:#x}, store +{:#x}, player at +{player_offset:#x}",
            module.rva(tick),
            module.rva(send_rename),
            module.rva(store)
        );
    }
    Ok(())
}
