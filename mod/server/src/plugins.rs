//! Mod DLLs in `shroudlink_mods/` next to the server executable (see the
//! `shroudlink-sdk` crate for the ABI). Each becomes a regular `ServerMod`.
//!
//! Mods are loaded from shadow copies in `shroudlink_mods/.loaded/`, so the DLL in
//! `shroudlink_mods/` can be overwritten while the server runs; a changed DLL is loaded
//! again within seconds (hot reload). The old copy stays loaded but unused.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use chestsort_common::log;
use serde_json::{json, Value};
use shroudlink_sdk::{HostApi, InventoryEvent, ModDecl, ModDeclV1, ABI_VERSION};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

use crate::game_api;
use crate::link_host::Host;
use crate::mods::{self, ClientId, ServerMod};

pub const DIR: &str = "shroudlink_mods";
const SHADOW_DIR: &str = ".loaded";
const RELOAD_CHECK: Duration = Duration::from_secs(2);

/// `ctx` is the network `Host` inside message callbacks, null inside inventory events
/// (the message is queued and sent by the next network update).
unsafe extern "C" fn host_send(ctx: *mut c_void, client: u64, json: *const u8, len: usize) {
    match serde_json::from_slice::<Value>(std::slice::from_raw_parts(json, len)) {
        Ok(message) => match (ctx as *mut Host).as_mut() {
            Some(host) => host.send(ClientId(client), &message),
            None => mods::queue(ClientId(client), message),
        },
        Err(e) => log!("shroudlink: mod sent invalid JSON ({e})"),
    }
}

unsafe extern "C" fn host_log(text: *const u8, len: usize) {
    log!("{}", String::from_utf8_lossy(std::slice::from_raw_parts(text, len)));
}

static API: HostApi = HostApi {
    version: ABI_VERSION,
    send: host_send,
    log: host_log,
    slots: game_api::slots,
    is_player: game_api::is_player,
    chest_name: game_api::chest_name,
    max_stack: game_api::max_stack,
    move_stack: game_api::move_stack,
};

enum Decl {
    V1(&'static ModDeclV1),
    V2(&'static ModDecl),
}

struct Plugin {
    decl: Decl,
    id: &'static str,
}

// the mod's state is only touched under the mod registry lock
unsafe impl Send for Plugin {}

impl Plugin {
    /// The v1 part every version has.
    fn v1(&self) -> &ModDeclV1 {
        match self.decl {
            Decl::V1(d) => d,
            // v2 starts with the v1 layout
            Decl::V2(d) => unsafe { &*(d as *const ModDecl as *const ModDeclV1) },
        }
    }
}

impl ServerMod for Plugin {
    fn id(&self) -> &'static str {
        self.id
    }
    fn describe(&self) -> Value {
        json!({ "id": self.id, "plugin": true })
    }
    fn on_join(&mut self, host: &mut Host, client: ClientId) {
        let d = self.v1();
        unsafe { (d.on_join)(d.state, &API, host as *mut Host as *mut c_void, client.0) }
    }
    fn on_message(&mut self, host: &mut Host, client: ClientId, message: &Value) {
        let text = message.to_string();
        let d = self.v1();
        unsafe { (d.on_message)(d.state, &API, host as *mut Host as *mut c_void, client.0, text.as_ptr(), text.len()) }
    }
    fn on_leave(&mut self, host: &mut Host, client: ClientId) {
        let d = self.v1();
        unsafe { (d.on_leave)(d.state, &API, host as *mut Host as *mut c_void, client.0) }
    }
    fn keep(&mut self, game: *mut c_void, event: &InventoryEvent) -> Vec<u64> {
        let Decl::V2(d) = self.decl else { return Vec::new() };
        let mut out = vec![0u64; 64];
        let n = unsafe { (d.keep)(d.state, &API, game, event, out.as_mut_ptr(), out.len()) };
        if n > out.len() {
            out.resize(n, 0);
            unsafe { (d.keep)(d.state, &API, game, event, out.as_mut_ptr(), out.len()) };
        }
        out.truncate(n);
        out
    }
    fn on_transfer(&mut self, game: *mut c_void, event: &InventoryEvent) -> u32 {
        let Decl::V2(d) = self.decl else { return 0 };
        unsafe { (d.on_transfer)(d.state, &API, game, event) }
    }
}

unsafe fn load(path: &Path) -> Result<Plugin, String> {
    let wide: Vec<u16> = path.as_os_str().to_string_lossy().encode_utf16().chain([0]).collect();
    let module = LoadLibraryW(wide.as_ptr());
    if module.is_null() {
        return Err("cannot load".into());
    }
    let decl = if let Some(entry) = GetProcAddress(module, b"shroudlink_mod_v2\0".as_ptr()) {
        let entry: extern "C" fn() -> *const ModDecl = std::mem::transmute(entry);
        let decl = entry().as_ref().ok_or("shroudlink_mod_v2 returned null")?;
        if decl.version != 2 {
            return Err(format!("ABI version {} (host {ABI_VERSION})", decl.version));
        }
        (decl.on_load)(decl.state, &API);
        Decl::V2(decl)
    } else {
        let entry = GetProcAddress(module, b"shroudlink_mod_v1\0".as_ptr()).ok_or("no shroudlink_mod_v2/v1 export")?;
        let entry: extern "C" fn() -> *const ModDeclV1 = std::mem::transmute(entry);
        let decl = entry().as_ref().ok_or("shroudlink_mod_v1 returned null")?;
        if decl.version != 1 {
            return Err(format!("ABI version {} (expected 1)", decl.version));
        }
        Decl::V1(decl)
    };
    let mut plugin = Plugin { decl, id: "" };
    let d = plugin.v1();
    let mut len = 0usize;
    let ptr = (d.id)(d.state, &mut len);
    plugin.id = std::str::from_utf8(std::slice::from_raw_parts(ptr, len)).map_err(|_| "id is not UTF-8")?;
    Ok(plugin)
}

/// A mod DLL in `shroudlink_mods/` and the version of it that is loaded.
struct Source {
    path: PathBuf,
    modified: Option<SystemTime>,
}

struct Loader {
    dir: PathBuf,
    sources: Vec<Source>,
    copies: u32,
    checked: Instant,
}

static LOADER: Mutex<Option<Loader>> = Mutex::new(None);

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl Loader {
    /// Copies the DLL to a fresh shadow file and loads that.
    fn load_copy(&mut self, path: &Path) -> Result<Plugin, String> {
        let shadow = self.dir.join(SHADOW_DIR);
        std::fs::create_dir_all(&shadow).map_err(|e| format!("cannot create {}: {e}", shadow.display()))?;
        self.copies += 1;
        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
        let copy = shadow.join(format!("{stem}.{}.{}.dll", std::process::id(), self.copies));
        std::fs::copy(path, &copy).map_err(|e| format!("cannot copy: {e}"))?;
        unsafe { load(&copy) }
    }

    fn dlls(&self) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return Vec::new() };
        let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("dll"))).collect();
        paths.sort();
        paths
    }
}

/// Loads every mod DLL of `dir` (missing directory: nothing to do).
pub fn load_all(dir: &Path) {
    // shadow copies of earlier runs are no longer loaded by anyone
    if let Ok(old) = std::fs::read_dir(dir.join(SHADOW_DIR)) {
        for entry in old.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    let mut loader = Loader { dir: dir.to_path_buf(), sources: Vec::new(), copies: 0, checked: Instant::now() };
    for path in loader.dlls() {
        let stamp = modified(&path);
        match loader.load_copy(&path) {
            Ok(plugin) => {
                log!("shroudlink: loaded mod '{}' from {}", plugin.id, path.display());
                mods::register(Box::new(plugin));
            }
            Err(e) => log!("shroudlink: skipped {}: {e}", path.display()),
        }
        loader.sources.push(Source { path, modified: stamp });
    }
    *LOADER.lock().unwrap_or_else(|e| e.into_inner()) = Some(loader);
}

/// Hot reload: loads mod DLLs that are new or changed since the last check.
pub fn reload_changed(host: &mut Host) {
    let mut guard = LOADER.lock().unwrap_or_else(|e| e.into_inner());
    let Some(loader) = guard.as_mut() else { return };
    if loader.checked.elapsed() < RELOAD_CHECK {
        return;
    }
    loader.checked = Instant::now();
    for path in loader.dlls() {
        let stamp = modified(&path);
        let known = loader.sources.iter().position(|s| s.path == path);
        if known.is_some_and(|i| loader.sources[i].modified == stamp) {
            continue;
        }
        // a file still being written fails to load; it is retried on the next check
        match loader.load_copy(&path) {
            Ok(plugin) => {
                log!("shroudlink: {} mod '{}' from {}", if known.is_some() { "reloaded" } else { "loaded" }, plugin.id, path.display());
                mods::replace(host, Box::new(plugin));
            }
            Err(e) => {
                log!("shroudlink: could not load {}: {e}", path.display());
                continue;
            }
        }
        match known {
            Some(i) => loader.sources[i].modified = stamp,
            None => loader.sources.push(Source { path, modified: stamp }),
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
        assert!(matches!(plugin.decl, Decl::V2(_)), "built against SDK v2");
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
        // outside an inventory event the game API answers nothing
        let event = InventoryEvent { kind: shroudlink_sdk::TransferKind::MoveAll, from: 1, to: 2 };
        assert!(plugin.keep(std::ptr::null_mut(), &event).is_empty());
        plugin.on_leave(&mut host, ClientId(nonce));
    }
}
