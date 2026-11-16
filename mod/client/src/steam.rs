//! The player's Steam name, for server mods that show players (the server itself only
//! knows entity ids). Read through Steam's flat API in the game's own `steam_api64.dll`:
//! `SteamAPI_SteamFriends_v018()` -> `SteamAPI_ISteamFriends_GetPersonaName(friends)`.

use std::ffi::{c_char, c_void, CStr};
use std::sync::OnceLock;

use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

/// Interface versions to try, newest first (the game ships v018).
const FRIENDS_ACCESSORS: [&[u8]; 3] = [b"SteamAPI_SteamFriends_v019\0", b"SteamAPI_SteamFriends_v018\0", b"SteamAPI_SteamFriends_v017\0"];

/// The Steam name, read once; None if Steam is not there (e.g. a non-Steam build).
pub fn persona_name() -> Option<String> {
    static NAME: OnceLock<Option<String>> = OnceLock::new();
    NAME.get_or_init(|| unsafe { read() }).clone()
}

unsafe fn read() -> Option<String> {
    let dll: Vec<u16> = "steam_api64.dll".encode_utf16().chain([0]).collect();
    let module = GetModuleHandleW(dll.as_ptr());
    if module.is_null() {
        return None;
    }
    let accessor = FRIENDS_ACCESSORS.iter().find_map(|name| GetProcAddress(module, name.as_ptr()))?;
    let accessor: extern "C" fn() -> *mut c_void = std::mem::transmute(accessor);
    let get_name = GetProcAddress(module, b"SteamAPI_ISteamFriends_GetPersonaName\0".as_ptr())?;
    let get_name: extern "C" fn(*mut c_void) -> *const c_char = std::mem::transmute(get_name);
    let friends = accessor();
    if friends.is_null() {
        return None;
    }
    let name = get_name(friends);
    if name.is_null() {
        return None;
    }
    let name = CStr::from_ptr(name).to_string_lossy().trim().to_string();
    (!name.is_empty()).then_some(name)
}
