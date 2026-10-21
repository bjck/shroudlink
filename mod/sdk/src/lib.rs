//! SDK for shroudlink server mods built as separate DLLs (see `docs/sdk-v2.md`).
//!
//! The host (the server DLL) loads every `*.dll` in `shroudlink_mods/` next to
//! `enshrouded_server.exe` and calls its `shroudlink_mod_v2` export. Messages to
//! client addons cross the DLL boundary as JSON text and game access goes through
//! a small C function table, so mods can be written in any language; Rust mods
//! implement [`Mod`] and use [`export_mod!`]:
//!
//! ```ignore
//! #[derive(Default)]
//! struct KeepTorches;
//! impl shroudlink_sdk::Mod for KeepTorches {
//!     fn id(&self) -> &'static str { "keep_torches" }
//!     // never quick stack torches out of a player's inventory
//!     fn keep(&mut self, game: &Game, event: &InventoryEvent) -> Vec<u64> {
//!         if !game.is_player(event.from) { return vec![]; }
//!         game.slots(event.from).iter().filter(|s| s.item == TORCH).map(|s| s.id).collect()
//!     }
//! }
//! shroudlink_sdk::export_mod!(KeepTorches);
//! ```

use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicPtr, Ordering};

pub use serde_json::{json, Value};
pub use shroudlink::ui;

mod event_types;
pub mod events;
pub use events::{GameEvent, Known};

/// ABI version of [`HostApi`] / [`ModDecl`]. The host also loads v1-v3 mods. `HostApi`
/// tells its own version: functions newer than it must not be called (the SDK checks).
pub const ABI_VERSION: u32 = 4;

/// One inventory slot. `id` is `page entity | slot index << 32` (the same on client and
/// server within a session); an empty slot has `count == 0`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Slot {
    pub id: u64,
    pub item: u32,
    pub count: u32,
}

impl Slot {
    /// The entity owning the slot's inventory page.
    pub fn page(&self) -> u32 {
        self.id as u32
    }
    pub fn index(&self) -> u8 {
        (self.id >> 32) as u8
    }
}

/// How the game is about to move a whole inventory.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferKind {
    /// Quick stack (hold R / F, Quick Stack Station): stacks of items the target holds.
    QuickStack = 1,
    /// Move all (tap R / F): every stack.
    MoveAll = 2,
}

/// An inventory transfer the game is about to make, from `from` into `to` (entity ids).
/// Either side can be the player: quick stack also runs from a chest into the player.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct InventoryEvent {
    pub kind: TransferKind,
    pub from: u32,
    pub to: u32,
}

/// Functions the host offers to mods. `ctx` and `game` are only valid during the
/// callback they were passed to. v2 appends the game functions to the v1 table.
#[repr(C)]
pub struct HostApi {
    pub version: u32,
    pub send: unsafe extern "C" fn(ctx: *mut c_void, client: u64, json: *const u8, len: usize),
    pub log: unsafe extern "C" fn(text: *const u8, len: usize),
    // v2: game access inside inventory callbacks
    /// Writes up to `cap` slots of the entity's inventory; returns how many it has.
    pub slots: unsafe extern "C" fn(game: *mut c_void, entity: u32, out: *mut Slot, cap: usize) -> usize,
    pub is_player: unsafe extern "C" fn(game: *mut c_void, entity: u32) -> bool,
    /// Copies up to `cap` bytes of the custom name; returns its full length (0: none).
    pub chest_name: unsafe extern "C" fn(game: *mut c_void, entity: u32, buf: *mut u8, cap: usize) -> usize,
    /// Stack limit of an item, 0 if unknown.
    pub max_stack: unsafe extern "C" fn(game: *mut c_void, item: u32) -> u32,
    /// Moves one stack into the entity's inventory; returns the number of items moved.
    pub move_stack: unsafe extern "C" fn(game: *mut c_void, slot: u64, to: u32) -> u32,
    // v4: the world, usable in any callback
    /// Writes up to `cap` player entity ids; returns how many players are in the world.
    pub players: unsafe extern "C" fn(out: *mut u32, cap: usize) -> usize,
    /// Position in metres into `out[0..3]`; false if the entity has none.
    pub position: unsafe extern "C" fn(entity: u32, out: *mut f64) -> bool,
    /// Moves an entity (a player sees itself there right away).
    pub teleport: unsafe extern "C" fn(entity: u32, x: f64, y: f64, z: f64) -> bool,
}

/// What a mod DLL exports through `shroudlink_mod_v2`. Strings are UTF-8 with a length.
/// The first fields are the v1 layout.
#[repr(C)]
pub struct ModDecl {
    pub version: u32,
    pub state: *mut c_void,
    pub id: unsafe extern "C" fn(state: *mut c_void, len: *mut usize) -> *const u8,
    pub on_join: unsafe extern "C" fn(state: *mut c_void, api: *const HostApi, ctx: *mut c_void, client: u64),
    pub on_message: unsafe extern "C" fn(state: *mut c_void, api: *const HostApi, ctx: *mut c_void, client: u64, json: *const u8, len: usize),
    pub on_leave: unsafe extern "C" fn(state: *mut c_void, api: *const HostApi, ctx: *mut c_void, client: u64),
    // v2
    pub on_load: unsafe extern "C" fn(state: *mut c_void, api: *const HostApi),
    /// Writes up to `cap` slot ids to keep; returns how many there are.
    pub keep: unsafe extern "C" fn(state: *mut c_void, api: *const HostApi, game: *mut c_void, event: *const InventoryEvent, out: *mut u64, cap: usize) -> usize,
    /// Moves stacks before the game does; returns the number of items moved.
    pub on_transfer: unsafe extern "C" fn(state: *mut c_void, api: *const HostApi, game: *mut c_void, event: *const InventoryEvent) -> u32,
    // v3
    /// Writes up to `cap` event type ids the mod wants; returns how many there are.
    pub game_events: unsafe extern "C" fn(state: *mut c_void, out: *mut u32, cap: usize) -> usize,
    /// One game event: its type id and payload.
    pub on_game_event: unsafe extern "C" fn(state: *mut c_void, api: *const HostApi, hash: u32, data: *const u8, len: usize),
}

/// The v2 export layout (v1 fields plus inventory events), for hosts that load v2 mods.
#[doc(hidden)]
#[repr(C)]
pub struct ModDeclV2 {
    pub version: u32,
    pub state: *mut c_void,
    pub id: unsafe extern "C" fn(state: *mut c_void, len: *mut usize) -> *const u8,
    pub on_join: unsafe extern "C" fn(state: *mut c_void, api: *const HostApi, ctx: *mut c_void, client: u64),
    pub on_message: unsafe extern "C" fn(state: *mut c_void, api: *const HostApi, ctx: *mut c_void, client: u64, json: *const u8, len: usize),
    pub on_leave: unsafe extern "C" fn(state: *mut c_void, api: *const HostApi, ctx: *mut c_void, client: u64),
    pub on_load: unsafe extern "C" fn(state: *mut c_void, api: *const HostApi),
    pub keep: unsafe extern "C" fn(state: *mut c_void, api: *const HostApi, game: *mut c_void, event: *const InventoryEvent, out: *mut u64, cap: usize) -> usize,
    pub on_transfer: unsafe extern "C" fn(state: *mut c_void, api: *const HostApi, game: *mut c_void, event: *const InventoryEvent) -> u32,
}

/// The v1 export layout, for hosts that still load v1 mods.
#[doc(hidden)]
#[repr(C)]
pub struct ModDeclV1 {
    pub version: u32,
    pub state: *mut c_void,
    pub id: unsafe extern "C" fn(state: *mut c_void, len: *mut usize) -> *const u8,
    pub on_join: unsafe extern "C" fn(state: *mut c_void, api: *const HostApi, ctx: *mut c_void, client: u64),
    pub on_message: unsafe extern "C" fn(state: *mut c_void, api: *const HostApi, ctx: *mut c_void, client: u64, json: *const u8, len: usize),
    pub on_leave: unsafe extern "C" fn(state: *mut c_void, api: *const HostApi, ctx: *mut c_void, client: u64),
}

/// Handle to the host inside a message callback.
pub struct Host<'a> {
    api: &'a HostApi,
    ctx: *mut c_void,
}

impl Host<'_> {
    /// The world: players, positions, teleporting.
    pub fn world(&self) -> World<'_> {
        World { api: self.api }
    }

    /// Sends a JSON message to a client addon (route it with `"mod"`; `"ui"` messages open windows).
    pub fn send(&mut self, client: u64, message: &Value) {
        let text = message.to_string();
        unsafe { (self.api.send)(self.ctx, client, text.as_ptr(), text.len()) }
    }

    /// Writes a line to the server log.
    pub fn log(&self, text: &str) {
        unsafe { (self.api.log)(text.as_ptr(), text.len()) }
    }
}

/// The world, from any callback (host v4; older hosts answer nothing).
pub struct World<'a> {
    api: &'a HostApi,
}

impl World<'_> {
    fn available(&self) -> bool {
        self.api.version >= 4
    }

    /// Entity ids of the players in the world.
    pub fn players(&self) -> Vec<u32> {
        if !self.available() {
            return Vec::new();
        }
        let mut out = vec![0u32; 64];
        let n = unsafe { (self.api.players)(out.as_mut_ptr(), out.len()) };
        out.truncate(n.min(64));
        out
    }

    /// Position in metres (x, y up, z).
    pub fn position(&self, entity: u32) -> Option<[f64; 3]> {
        if !self.available() {
            return None;
        }
        let mut out = [0f64; 3];
        unsafe { (self.api.position)(entity, out.as_mut_ptr()) }.then_some(out)
    }

    /// Moves an entity to `to` (metres); a player sees itself there right away.
    pub fn teleport(&self, entity: u32, to: [f64; 3]) -> bool {
        self.available() && unsafe { (self.api.teleport)(entity, to[0], to[1], to[2]) }
    }
}

/// Game access inside an inventory callback.
pub struct Game<'a> {
    api: &'a HostApi,
    game: *mut c_void,
}

impl Game<'_> {
    pub fn world(&self) -> World<'_> {
        World { api: self.api }
    }

    /// Every inventory slot of an entity (pages x 8 slots, empty ones included).
    pub fn slots(&self, entity: u32) -> Vec<Slot> {
        let mut out = vec![Slot::default(); 256];
        loop {
            let n = unsafe { (self.api.slots)(self.game, entity, out.as_mut_ptr(), out.len()) };
            if n <= out.len() {
                out.truncate(n);
                return out;
            }
            out.resize(n, Slot::default());
        }
    }

    pub fn is_player(&self, entity: u32) -> bool {
        unsafe { (self.api.is_player)(self.game, entity) }
    }

    /// The name given to an object with the Rename button.
    pub fn chest_name(&self, entity: u32) -> Option<String> {
        let mut buf = vec![0u8; 256];
        let n = unsafe { (self.api.chest_name)(self.game, entity, buf.as_mut_ptr(), buf.len()) };
        if n > buf.len() {
            buf.resize(n, 0);
            unsafe { (self.api.chest_name)(self.game, entity, buf.as_mut_ptr(), buf.len()) };
        }
        (n != 0).then(|| String::from_utf8_lossy(&buf[..n]).into_owned())
    }

    pub fn max_stack(&self, item: u32) -> Option<u32> {
        Some(unsafe { (self.api.max_stack)(self.game, item) }).filter(|&n| n != 0)
    }

    /// Moves one stack into `to`'s inventory (partial stacks of the same item first, then
    /// empty slots). Returns how many items moved; kept slots never move.
    pub fn move_stack(&self, slot: u64, to: u32) -> u32 {
        unsafe { (self.api.move_stack)(self.game, slot, to) }
    }

    /// Queues a message to a client addon; it is sent after the event.
    pub fn send(&self, client: u64, message: &Value) {
        let text = message.to_string();
        unsafe { (self.api.send)(std::ptr::null_mut(), client, text.as_ptr(), text.len()) }
    }
}

/// Sends messages to client addons from a game event callback (delivered right after it).
pub struct Events<'a> {
    api: &'a HostApi,
}

impl Events<'_> {
    pub fn world(&self) -> World<'_> {
        World { api: self.api }
    }

    pub fn send(&self, client: u64, message: &Value) {
        let text = message.to_string();
        unsafe { (self.api.send)(std::ptr::null_mut(), client, text.as_ptr(), text.len()) }
    }
}

/// A server mod. `client` identifies a connected addon session.
pub trait Mod: Send {
    fn id(&self) -> &'static str;
    /// Once, right after the DLL was loaded.
    fn on_load(&mut self) {}
    fn on_join(&mut self, _host: &mut Host, _client: u64) {}
    fn on_message(&mut self, _host: &mut Host, _client: u64, _message: &Value) {}
    fn on_leave(&mut self, _host: &mut Host, _client: u64) {}
    /// Slots of `event.from` that must not move in this transfer.
    fn keep(&mut self, _game: &Game, _event: &InventoryEvent) -> Vec<u64> {
        Vec::new()
    }
    /// Moves stacks itself (with [`Game::move_stack`]) before the game's own transfer;
    /// returns the number of items moved.
    fn on_transfer(&mut self, _game: &Game, _event: &InventoryEvent) -> u32 {
        0
    }
    /// Game event types to receive (names from [`events::EVENT_TYPES`], e.g. "EntityDiedEvent").
    fn game_events(&self) -> Vec<&'static str> {
        Vec::new()
    }
    /// One game event of a type from [`Mod::game_events`]. Runs on the game thread once per
    /// event, so keep it short.
    fn on_game_event(&mut self, _events: &Events, _event: &GameEvent) {}
}

static API: AtomicPtr<HostApi> = AtomicPtr::new(std::ptr::null_mut());

/// Writes a line to the server log (works once the mod is loaded).
pub fn log(text: &str) {
    let api = API.load(Ordering::Relaxed);
    if !api.is_null() {
        unsafe { ((*api).log)(text.as_ptr(), text.len()) }
    }
}

/// `format!`-style [`log`].
#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => { $crate::log(&format!($($arg)*)) };
}

/// The server's folder (next to `enshrouded_server.exe`), for a mod's own files.
pub fn server_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())).unwrap_or_else(|| PathBuf::from("."))
}

#[doc(hidden)]
pub mod glue {
    use super::*;

    pub fn decl<M: Mod + 'static>(m: M) -> ModDecl {
        let state = Box::into_raw(Box::new(Box::new(m) as Box<dyn Mod>)) as *mut c_void;
        ModDecl { version: ABI_VERSION, state, id, on_join, on_message, on_leave, on_load, keep, on_transfer, game_events, on_game_event }
    }

    unsafe fn this<'a>(state: *mut c_void) -> &'a mut Box<dyn Mod> {
        &mut *(state as *mut Box<dyn Mod>)
    }

    unsafe extern "C" fn id(state: *mut c_void, len: *mut usize) -> *const u8 {
        let id = this(state).id();
        *len = id.len();
        id.as_ptr()
    }

    unsafe extern "C" fn on_load(state: *mut c_void, api: *const HostApi) {
        API.store(api as *mut HostApi, Ordering::Relaxed);
        this(state).on_load();
    }

    unsafe extern "C" fn on_join(state: *mut c_void, api: *const HostApi, ctx: *mut c_void, client: u64) {
        this(state).on_join(&mut Host { api: &*api, ctx }, client);
    }

    unsafe extern "C" fn on_message(state: *mut c_void, api: *const HostApi, ctx: *mut c_void, client: u64, json: *const u8, len: usize) {
        let text = std::slice::from_raw_parts(json, len);
        if let Ok(message) = serde_json::from_slice::<Value>(text) {
            this(state).on_message(&mut Host { api: &*api, ctx }, client, &message);
        }
    }

    unsafe extern "C" fn on_leave(state: *mut c_void, api: *const HostApi, ctx: *mut c_void, client: u64) {
        this(state).on_leave(&mut Host { api: &*api, ctx }, client);
    }

    unsafe extern "C" fn keep(state: *mut c_void, api: *const HostApi, game: *mut c_void, event: *const InventoryEvent, out: *mut u64, cap: usize) -> usize {
        let slots = this(state).keep(&Game { api: &*api, game }, &*event);
        std::ptr::copy_nonoverlapping(slots.as_ptr(), out, slots.len().min(cap));
        slots.len()
    }

    unsafe extern "C" fn on_transfer(state: *mut c_void, api: *const HostApi, game: *mut c_void, event: *const InventoryEvent) -> u32 {
        this(state).on_transfer(&Game { api: &*api, game }, &*event)
    }

    unsafe extern "C" fn game_events(state: *mut c_void, out: *mut u32, cap: usize) -> usize {
        let names = this(state).game_events();
        let hashes: Vec<u32> = names
            .iter()
            .filter_map(|n| {
                let hash = events::hash_of(n);
                if hash.is_none() {
                    log(&format!("unknown game event type '{n}'"));
                }
                hash
            })
            .collect();
        std::ptr::copy_nonoverlapping(hashes.as_ptr(), out, hashes.len().min(cap));
        hashes.len()
    }

    unsafe extern "C" fn on_game_event(state: *mut c_void, api: *const HostApi, hash: u32, data: *const u8, len: usize) {
        let Some(name) = events::name_of(hash) else { return };
        let event = GameEvent { hash, name, data: std::slice::from_raw_parts(data, len) };
        this(state).on_game_event(&Events { api: &*api }, &event);
    }
}

/// Exports `shroudlink_mod_v2` (the declaration carries ABI version 3) for a [`Mod`] type
/// implementing `Default`.
#[macro_export]
macro_rules! export_mod {
    ($t:ty) => {
        #[no_mangle]
        pub extern "C" fn shroudlink_mod_v2() -> *const $crate::ModDecl {
            static DECL: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
            *DECL.get_or_init(|| Box::into_raw(Box::new($crate::glue::decl(<$t>::default()))) as usize) as *const $crate::ModDecl
        }
    };
}

/// Unit-testing mods without the game.
pub mod testing {
    use super::*;

    /// A host that records what a mod sends and has an inventory-less game.
    pub struct MockHost {
        api: HostApi,
        sent: Box<Vec<(u64, Value)>>,
    }

    unsafe extern "C" fn send(ctx: *mut c_void, client: u64, json: *const u8, len: usize) {
        if let (Some(sent), Ok(value)) = ((ctx as *mut Vec<(u64, Value)>).as_mut(), serde_json::from_slice(std::slice::from_raw_parts(json, len))) {
            sent.push((client, value));
        }
    }
    unsafe extern "C" fn log(text: *const u8, len: usize) {
        eprintln!("{}", String::from_utf8_lossy(std::slice::from_raw_parts(text, len)));
    }
    unsafe extern "C" fn slots(_: *mut c_void, _: u32, _: *mut Slot, _: usize) -> usize {
        0
    }
    unsafe extern "C" fn is_player(_: *mut c_void, _: u32) -> bool {
        false
    }
    unsafe extern "C" fn chest_name(_: *mut c_void, _: u32, _: *mut u8, _: usize) -> usize {
        0
    }
    unsafe extern "C" fn max_stack(_: *mut c_void, _: u32) -> u32 {
        0
    }
    unsafe extern "C" fn move_stack(_: *mut c_void, _: u64, _: u32) -> u32 {
        0
    }
    unsafe extern "C" fn players(_: *mut u32, _: usize) -> usize {
        0
    }
    unsafe extern "C" fn position(_: u32, _: *mut f64) -> bool {
        false
    }
    unsafe extern "C" fn teleport(_: u32, _: f64, _: f64, _: f64) -> bool {
        false
    }

    impl Default for MockHost {
        fn default() -> Self {
            MockHost {
                api: HostApi { version: ABI_VERSION, send, log, slots, is_player, chest_name, max_stack, move_stack, players, position, teleport },
                sent: Box::default(),
            }
        }
    }

    impl MockHost {
        /// A [`Host`] to pass to a mod's message callbacks.
        pub fn host(&mut self) -> Host<'_> {
            let ctx = &mut *self.sent as *mut Vec<(u64, Value)> as *mut c_void;
            Host { api: &self.api, ctx }
        }

        /// Messages sent since the last call, with their client.
        pub fn take_sent(&mut self) -> Vec<(u64, Value)> {
            std::mem::take(&mut *self.sent)
        }
    }
}
