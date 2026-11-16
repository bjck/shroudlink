//! The player's Steam name and Steam ID, for server mods that show players or keep
//! progress per account (the server itself only knows entity ids, which change every
//! session). Read through Steam's flat API in the game's own `steam_api64.dll`:
//! `SteamAPI_SteamFriends_v018()` -> `SteamAPI_ISteamFriends_GetPersonaName(friends)`,
//! `SteamAPI_SteamUser_v023()` -> `SteamAPI_ISteamUser_GetSteamID(user)` (SteamID64).

use std::ffi::{c_char, c_void, CStr};
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

/// Interface versions to try, newest first (the game ships v018).
const FRIENDS_ACCESSORS: [&[u8]; 3] = [b"SteamAPI_SteamFriends_v019\0", b"SteamAPI_SteamFriends_v018\0", b"SteamAPI_SteamFriends_v017\0"];
/// The game ships v023.
const USER_ACCESSORS: [&[u8]; 3] = [b"SteamAPI_SteamUser_v023\0", b"SteamAPI_SteamUser_v022\0", b"SteamAPI_SteamUser_v021\0"];

/// The Steam name, read once; None if Steam is not there (e.g. a non-Steam build).
pub fn persona_name() -> Option<String> {
    static NAME: OnceLock<Option<String>> = OnceLock::new();
    NAME.get_or_init(|| unsafe { read() }).clone()
}

/// The Steam ID (SteamID64), read once; None without Steam.
pub fn steam_id() -> Option<u64> {
    static ID: OnceLock<Option<u64>> = OnceLock::new();
    *ID.get_or_init(|| unsafe { read_id() })
}

unsafe fn steam_module() -> Option<HMODULE> {
    let dll: Vec<u16> = "steam_api64.dll".encode_utf16().chain([0]).collect();
    let module = GetModuleHandleW(dll.as_ptr());
    (!module.is_null()).then_some(module)
}

unsafe fn read() -> Option<String> {
    let module = steam_module()?;
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

unsafe fn read_id() -> Option<u64> {
    let module = steam_module()?;
    let accessor = USER_ACCESSORS.iter().find_map(|name| GetProcAddress(module, name.as_ptr()))?;
    let accessor: extern "C" fn() -> *mut c_void = std::mem::transmute(accessor);
    let get_id = GetProcAddress(module, b"SteamAPI_ISteamUser_GetSteamID\0".as_ptr())?;
    let get_id: extern "C" fn(*mut c_void) -> u64 = std::mem::transmute(get_id);
    let user = accessor();
    if user.is_null() {
        return None;
    }
    Some(get_id(user)).filter(|&id| id != 0)
}
