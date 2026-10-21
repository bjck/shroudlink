//! Mod DLLs in `shroudlink_mods/` next to the server executable (see the
//! `shroudlink-sdk` crate for the ABI). Each becomes a regular `ServerMod`.

use std::ffi::c_void;
use std::path::Path;

use chestsort_common::log;
use serde_json::{json, Value};
use shroudlink_sdk::{HostApi, ModDecl, ABI_VERSION};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

use crate::link_host::Host;
use crate::mods::{self, ClientId, ServerMod};

pub const DIR: &str = "shroudlink_mods";

unsafe extern "C" fn host_send(ctx: *mut c_void, client: u64, json: *const u8, len: usize) {
    let host = &mut *(ctx as *mut Host);
    match serde_json::from_slice::<Value>(std::slice::from_raw_parts(json, len)) {
        Ok(message) => host.send(ClientId(client), &message),
        Err(e) => log!("shroudlink: mod sent invalid JSON ({e})"),
    }
}

unsafe extern "C" fn host_log(text: *const u8, len: usize) {
    log!("{}", String::from_utf8_lossy(std::slice::from_raw_parts(text, len)));
}

static API: HostApi = HostApi { version: ABI_VERSION, send: host_send, log: host_log };

struct Plugin {
    decl: &'static ModDecl,
    id: &'static str,
}

// the mod's state is only touched from the host's network update
unsafe impl Send for Plugin {}

impl ServerMod for Plugin {
    fn id(&self) -> &'static str {
        self.id
    }
    fn describe(&self) -> Value {
        json!({ "id": self.id, "plugin": true })
    }
    fn on_join(&mut self, host: &mut Host, client: ClientId) {
        unsafe { (self.decl.on_join)(self.decl.state, &API, host as *mut Host as *mut c_void, client.0) }
    }
    fn on_message(&mut self, host: &mut Host, client: ClientId, message: &Value) {
        let text = message.to_string();
        unsafe { (self.decl.on_message)(self.decl.state, &API, host as *mut Host as *mut c_void, client.0, text.as_ptr(), text.len()) }
    }
    fn on_leave(&mut self, host: &mut Host, client: ClientId) {
        unsafe { (self.decl.on_leave)(self.decl.state, &API, host as *mut Host as *mut c_void, client.0) }
    }
}

unsafe fn load(path: &Path) -> Result<Plugin, String> {
    let wide: Vec<u16> = path.as_os_str().to_string_lossy().encode_utf16().chain([0]).collect();
    let module = LoadLibraryW(wide.as_ptr());
    if module.is_null() {
        return Err("cannot load".into());
    }
    let entry = GetProcAddress(module, b"shroudlink_mod_v1\0".as_ptr()).ok_or("no shroudlink_mod_v1 export")?;
    let entry: extern "C" fn() -> *const ModDecl = std::mem::transmute(entry);
    let decl = entry().as_ref().ok_or("shroudlink_mod_v1 returned null")?;
    if decl.version != ABI_VERSION {
        return Err(format!("ABI version {} (host {ABI_VERSION})", decl.version));
    }
    let mut len = 0usize;
    let ptr = (decl.id)(decl.state, &mut len);
    let id = std::str::from_utf8(std::slice::from_raw_parts(ptr, len)).map_err(|_| "id is not UTF-8")?;
    Ok(Plugin { decl, id })
}

/// Loads every mod DLL of `dir` (missing directory: nothing to do).
pub fn load_all(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("dll"))).collect();
    paths.sort();
    for path in paths {
        match unsafe { load(&path) } {
            Ok(plugin) => {
                log!("shroudlink: loaded mod '{}' from {}", plugin.id, path.display());
                mods::register(Box::new(plugin));
            }
            Err(e) => log!("shroudlink: skipped {}: {e}", path.display()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Loads the example mod DLL built in the same workspace and drives it through the host.
    #[test]
    fn example_plugin_round_trip() {
        let dll = std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().join("hello_mod.dll");
        if !dll.exists() {
            eprintln!("skipped: build hello_mod first ({})", dll.display());
            return;
        }
        let mut plugin = unsafe { load(&dll) }.expect("loads");
        assert_eq!(plugin.id(), "hello");
        let mut host = Host::default();
        let nonce = 0x42;
        host.connect_for_test(4, nonce);
        plugin.on_join(&mut host, ClientId(nonce));
        let click = json!({ "mod": "hello", "op": "ui_event", "event": { "window": "hello", "node": "click", "event": { "kind": "click" } } });
        plugin.on_message(&mut host, ClientId(nonce), &click);
        plugin.on_message(&mut host, ClientId(nonce), &click);
        let sent: Vec<Value> = host.received_for_test(4, nonce).into_iter().filter(|m| m["owner"] == "hello").collect();
        assert_eq!(sent[0]["op"], "entries");
        let last = sent.last().unwrap();
        assert_eq!(last["op"], "open");
        assert!(last["window"]["content"].to_string().contains("clicked 2 time(s)"));
        let nonce = ClientId(nonce);
        plugin.on_leave(&mut host, nonce);
    }
}
