//! SDK for shroudlink server mods built as separate DLLs.
//!
//! The host (the server DLL) loads every `*.dll` in `shroudlink_mods/` next to
//! `enshrouded_server.exe` and calls its `shroudlink_mod_v1` export. Messages
//! cross the DLL boundary as JSON text, so mods can be written against this
//! small C ABI in any language; Rust mods implement [`Mod`] and use
//! [`export_mod!`]:
//!
//! ```ignore
//! #[derive(Default)]
//! struct Hello;
//! impl shroudlink_sdk::Mod for Hello {
//!     fn id(&self) -> &'static str { "hello" }
//!     fn on_message(&mut self, host: &mut shroudlink_sdk::Host, client: u64, message: &serde_json::Value) {
//!         host.send(client, &serde_json::json!({ "mod": "hello", "echo": message }));
//!     }
//! }
//! shroudlink_sdk::export_mod!(Hello);
//! ```

use std::ffi::c_void;

pub use serde_json::{json, Value};
pub use shroudlink::ui;

/// ABI version of [`HostApi`] / [`ModDecl`]; the host refuses other versions.
pub const ABI_VERSION: u32 = 1;

/// Functions the host offers to mods. `ctx` is only valid during the callback it was passed to.
#[repr(C)]
pub struct HostApi {
    pub version: u32,
    pub send: unsafe extern "C" fn(ctx: *mut c_void, client: u64, json: *const u8, len: usize),
    pub log: unsafe extern "C" fn(text: *const u8, len: usize),
}

/// What a mod DLL exports through `shroudlink_mod_v1`. All strings are UTF-8 with a length.
#[repr(C)]
pub struct ModDecl {
    pub version: u32,
    pub state: *mut c_void,
    pub id: unsafe extern "C" fn(state: *mut c_void, len: *mut usize) -> *const u8,
    pub on_join: unsafe extern "C" fn(state: *mut c_void, api: *const HostApi, ctx: *mut c_void, client: u64),
    pub on_message: unsafe extern "C" fn(state: *mut c_void, api: *const HostApi, ctx: *mut c_void, client: u64, json: *const u8, len: usize),
    pub on_leave: unsafe extern "C" fn(state: *mut c_void, api: *const HostApi, ctx: *mut c_void, client: u64),
}

/// Handle to the host inside a callback.
pub struct Host<'a> {
    api: &'a HostApi,
    ctx: *mut c_void,
}

impl Host<'_> {
    /// Sends a JSON message to a client addon (route it with `"mod"`; `"ui"` messages open windows).
    pub fn send(&mut self, client: u64, message: &Value) {
        let text = message.to_string();
        unsafe { (self.api.send)(self.ctx, client, text.as_ptr(), text.len()) }
    }

    /// Writes a line to the host log (`chestsort.log`).
    pub fn log(&self, text: &str) {
        unsafe { (self.api.log)(text.as_ptr(), text.len()) }
    }
}

/// A server mod. `client` identifies a connected addon session.
pub trait Mod: Send {
    fn id(&self) -> &'static str;
    fn on_join(&mut self, _host: &mut Host, _client: u64) {}
    fn on_message(&mut self, host: &mut Host, client: u64, message: &Value);
    fn on_leave(&mut self, _host: &mut Host, _client: u64) {}
}

#[doc(hidden)]
pub mod glue {
    use super::*;

    pub fn decl<M: Mod + 'static>(m: M) -> ModDecl {
        let state = Box::into_raw(Box::new(Box::new(m) as Box<dyn Mod>)) as *mut c_void;
        ModDecl { version: ABI_VERSION, state, id, on_join, on_message, on_leave }
    }

    unsafe fn this<'a>(state: *mut c_void) -> &'a mut Box<dyn Mod> {
        &mut *(state as *mut Box<dyn Mod>)
    }

    unsafe extern "C" fn id(state: *mut c_void, len: *mut usize) -> *const u8 {
        let id = this(state).id();
        *len = id.len();
        id.as_ptr()
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
}

/// Exports `shroudlink_mod_v1` for a [`Mod`] type implementing `Default`.
#[macro_export]
macro_rules! export_mod {
    ($t:ty) => {
        #[no_mangle]
        pub extern "C" fn shroudlink_mod_v1() -> *const $crate::ModDecl {
            static DECL: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
            *DECL.get_or_init(|| Box::into_raw(Box::new($crate::glue::decl(<$t>::default()))) as usize) as *const $crate::ModDecl
        }
    };
}
