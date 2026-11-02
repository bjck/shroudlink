//! Singleplayer: the game client hosts the world itself, so the shroudlink core can run
//! inside `enshrouded.exe`. If `shroudlink_core.dll` lies next to the game, the addon
//! loads it as soon as the game code is decrypted; the core then hooks the client's copy
//! of the server functions and loads `shroudlink_mods/` like a dedicated server does.
//! Without the file nothing changes: the addon only talks to servers.

use std::os::windows::ffi::OsStrExt;

use chestsort_common::log;
use windows_sys::Win32::System::LibraryLoader::LoadLibraryW;

/// The core's file name next to `enshrouded.exe` (the same DLL a server installs as
/// `steam_api64.dll`; under this name it does not act as a Steam proxy).
pub const CORE_FILE: &str = "shroudlink_core.dll";

/// Loads the core once per process, if it is installed.
pub fn load_core() {
    let path = chestsort_common::game_dir().join(CORE_FILE);
    if !path.exists() {
        return;
    }
    // a hot-reloaded addon copy must not load the core a second time
    if std::env::var_os("SHROUDLINK_CORE_LOADED").is_some() {
        return;
    }
    std::env::set_var("SHROUDLINK_CORE_LOADED", "1");
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let module = unsafe { LoadLibraryW(wide.as_ptr()) };
    if module.is_null() {
        log!("singleplayer: could not load {} (error {})", path.display(), std::io::Error::last_os_error());
    } else {
        log!("singleplayer: loaded the shroudlink core from {}", path.display());
    }
}
