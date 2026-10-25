//! Keyboard input for mod text fields.
//!
//! The game window's message procedure is subclassed. While a mod text field has
//! focus, typed characters and editing keys go to that field and are kept away
//! from the game (otherwise typing "j" would switch to the journal tab).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicU32, Ordering};
use std::sync::Mutex;

use chestsort_common::log;
use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, TRUE, WPARAM};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_BACK, VK_ESCAPE, VK_RETURN};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, DefWindowProcW, EnumWindows, GetWindowLongPtrW, GetWindowThreadProcessId, IsWindowVisible, SetWindowLongPtrW,
    GWLP_WNDPROC, WM_CHAR, WM_INPUT, WM_KEYDOWN, WM_SYSCHAR, WM_SYSKEYDOWN,
};

/// What the focused text field receives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Key {
    Char(char),
    Backspace,
    Enter,
    Escape,
}

static FOCUSED: AtomicBool = AtomicBool::new(false);

/// Mouse wheel turned since the last `take_wheel`, in notches * 120 (positive: away from
/// the user). From `WM_MOUSEWHEEL`, or from raw input if that message never comes.
static WHEEL: AtomicI32 = AtomicI32::new(0);
static RAW_WHEEL: AtomicI32 = AtomicI32::new(0);
static WHEEL_MESSAGES: AtomicBool = AtomicBool::new(false);
const WM_MOUSEWHEEL: u32 = 0x020a;

/// The key that opens the MODS page (a letter's virtual key; `shroudlink_keys.json`
/// `{"mods": "K"}` in the game folder changes it), and whether it was pressed.
static MODS_KEY: AtomicU32 = AtomicU32::new(0);
static MODS_KEY_PRESSED: AtomicBool = AtomicBool::new(false);

/// The letter of the MODS key (read once from `shroudlink_keys.json`, default K).
pub fn mods_key() -> char {
    let mut vk = MODS_KEY.load(Ordering::Relaxed);
    if vk == 0 {
        let chosen = std::fs::read_to_string(chestsort_common::game_dir().join("shroudlink_keys.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| v["mods"].as_str().and_then(|s| s.chars().next()))
            .map(|c| c.to_ascii_uppercase())
            .filter(char::is_ascii_uppercase)
            .unwrap_or('K');
        vk = chosen as u32;
        MODS_KEY.store(vk, Ordering::Relaxed);
    }
    char::from_u32(vk).unwrap_or('K')
}

/// True once after the MODS key was pressed (not while a text field has focus).
pub fn take_mods_key() -> bool {
    MODS_KEY_PRESSED.swap(false, Ordering::Relaxed)
}

/// Taps the game's key for the player menu's backpack tab (B, as its tab bar shows), as
/// the player would.
pub fn press_menu_key() {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE};
    const B_SCAN: u16 = 0x30;
    let key = |up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: 0, wScan: B_SCAN, dwFlags: KEYEVENTF_SCANCODE | if up { KEYEVENTF_KEYUP } else { 0 }, time: 0, dwExtraInfo: 0 } },
    };
    let inputs = [key(false), key(true)];
    unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
}

/// The wheel movement since the last call (the input is not taken from the game).
pub fn take_wheel() -> i32 {
    let raw = RAW_WHEEL.swap(0, Ordering::Relaxed);
    let legacy = WHEEL.swap(0, Ordering::Relaxed);
    if WHEEL_MESSAGES.load(Ordering::Relaxed) { legacy } else { raw }
}
/// While a search field is on screen, typing "/" jumps into it.
static SEARCH_HOTKEY: AtomicBool = AtomicBool::new(false);
static SEARCH_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Letters mods registered as slot keys (bit 0 = A).
static SLOT_KEYS: AtomicU32 = AtomicU32::new(0);
/// Set on menu screens, where slot keys act on the hovered backpack slot.
static SLOT_KEYS_ALLOWED: AtomicBool = AtomicBool::new(false);
/// A slot key press waiting for the hovered slot: the letter, plus 0x100 once one UI frame
/// has ended since. A press can arrive between the slots being drawn and the end of the
/// frame, so it stays valid through the whole next frame. 0 = none.
static PENDING_KEY: AtomicU32 = AtomicU32::new(0);

pub fn arm_slot_keys(mask: u32) {
    SLOT_KEYS.store(mask, Ordering::Relaxed);
}

pub fn allow_slot_keys(allowed: bool) {
    SLOT_KEYS_ALLOWED.store(allowed, Ordering::Relaxed);
}

/// The slot key pressed (once) while the cursor is inside `rect` (x, y, w, h, UI units).
pub fn slot_key_pressed_over(rect: [f32; 4], (x, y): (f32, f32)) -> Option<char> {
    let pending = PENDING_KEY.load(Ordering::Relaxed);
    if pending == 0 {
        return None;
    }
    let inside = x >= rect[0] && x < rect[0] + rect[2] && y >= rect[1] && y < rect[1] + rect[3];
    (inside && PENDING_KEY.swap(0, Ordering::Relaxed) != 0).then(|| (pending & 0xff) as u8 as char)
}

/// End of a UI frame: forgets a slot key press that a whole frame of slots did not take.
pub fn clear_slot_key() {
    let _ = PENDING_KEY.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |p| Some(if p != 0 && p & 0x100 == 0 { p | 0x100 } else { 0 }));
}

/// Enables the "/" hotkey for the frame's search field.
pub fn arm_search_hotkey(armed: bool) {
    SEARCH_HOTKEY.store(armed, Ordering::Relaxed);
}

/// True once after "/" was typed while the hotkey was armed.
pub fn search_requested() -> bool {
    SEARCH_REQUESTED.swap(false, Ordering::Relaxed)
}
static KEYS: Mutex<VecDeque<Key>> = Mutex::new(VecDeque::new());
static WINDOW: AtomicIsize = AtomicIsize::new(0);
static ORIGINAL: AtomicIsize = AtomicIsize::new(0);

/// Routes the keyboard to mod text fields (true) or back to the game (false).
pub fn set_focus(focused: bool) {
    if !focused {
        KEYS.lock().unwrap().clear();
    }
    FOCUSED.store(focused, Ordering::Relaxed);
}

/// Keys typed since the last call (only while focused).
pub fn take_keys() -> Vec<Key> {
    KEYS.lock().unwrap().drain(..).collect()
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_KEYDOWN && lparam & (1 << 30) == 0 && !FOCUSED.load(Ordering::Relaxed) && wparam as u32 == mods_key() as u32 {
        MODS_KEY_PRESSED.store(true, Ordering::Relaxed);
    }
    if msg == WM_MOUSEWHEEL {
        WHEEL_MESSAGES.store(true, Ordering::Relaxed);
        WHEEL.fetch_add((wparam >> 16) as u16 as i16 as i32, Ordering::Relaxed);
    } else if msg == WM_INPUT {
        RAW_WHEEL.fetch_add(raw_wheel(lparam), Ordering::Relaxed);
    }
    if !FOCUSED.load(Ordering::Relaxed) && SEARCH_HOTKEY.load(Ordering::Relaxed) && msg == WM_CHAR && wparam == '/' as usize {
        SEARCH_REQUESTED.store(true, Ordering::Relaxed);
        return 0;
    }
    let letter = (b'A' as usize..=b'Z' as usize).contains(&wparam).then(|| wparam as u32 - b'A' as u32);
    if let Some(bit) = letter.filter(|&b| SLOT_KEYS.load(Ordering::Relaxed) & 1 << b != 0) {
        if !FOCUSED.load(Ordering::Relaxed) && SLOT_KEYS_ALLOWED.load(Ordering::Relaxed) && msg == WM_KEYDOWN && lparam & (1 << 30) == 0 {
            // a fresh press (not auto-repeat); the slot under the cursor takes it
            PENDING_KEY.store(b'A' as u32 + bit, Ordering::Relaxed);
        }
    }
    if FOCUSED.load(Ordering::Relaxed) {
        match msg {
            WM_CHAR => {
                if let Some(c) = char::from_u32(wparam as u32).filter(|c| !c.is_control()) {
                    KEYS.lock().unwrap().push_back(Key::Char(c));
                }
                return 0;
            }
            WM_KEYDOWN => {
                let key = match wparam as u16 {
                    VK_BACK => Some(Key::Backspace),
                    VK_RETURN => Some(Key::Enter),
                    VK_ESCAPE => Some(Key::Escape),
                    _ => None,
                };
                if let Some(k) = key {
                    KEYS.lock().unwrap().push_back(k);
                }
                return 0;
            }
            WM_SYSKEYDOWN | WM_SYSCHAR => return 0,
            // raw keyboard presses stay away from the game (Windows still cleans up);
            // releases and all mouse input pass through
            WM_INPUT if is_raw_key_press(lparam) => return DefWindowProcW(hwnd, msg, wparam, lparam),
            // key releases always reach the game: had it seen the press (e.g. the "/" that
            // opened the search) but not the release, it would think the key is still held
            // and ignore the next presses (Esc no longer closed the chest)
            _ => {}
        }
    }
    let original: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT = std::mem::transmute(ORIGINAL.load(Ordering::Relaxed));
    CallWindowProcW(Some(original), hwnd, msg, wparam, lparam)
}

/// The wheel movement in a raw mouse input message (0 for anything else).
unsafe fn raw_wheel(lparam: LPARAM) -> i32 {
    use windows_sys::Win32::UI::Input::{GetRawInputData, HRAWINPUT, RAWINPUT, RAWINPUTHEADER, RID_INPUT, RIM_TYPEMOUSE};
    const RI_MOUSE_WHEEL: u16 = 0x0400;
    let mut raw: RAWINPUT = std::mem::zeroed();
    let mut size = std::mem::size_of::<RAWINPUT>() as u32;
    let got = GetRawInputData(lparam as HRAWINPUT, RID_INPUT, &mut raw as *mut RAWINPUT as *mut _, &mut size, std::mem::size_of::<RAWINPUTHEADER>() as u32);
    if got == u32::MAX || got == 0 || raw.header.dwType != RIM_TYPEMOUSE {
        return 0;
    }
    let buttons = raw.data.mouse.Anonymous.Anonymous;
    if buttons.usButtonFlags & RI_MOUSE_WHEEL != 0 { buttons.usButtonData as i16 as i32 } else { 0 }
}

/// True for a raw input message that is a keyboard key press (not a release, not mouse).
unsafe fn is_raw_key_press(lparam: LPARAM) -> bool {
    use windows_sys::Win32::UI::Input::{GetRawInputData, HRAWINPUT, RAWINPUT, RAWINPUTHEADER, RID_INPUT, RIM_TYPEKEYBOARD};
    use windows_sys::Win32::UI::WindowsAndMessaging::RI_KEY_BREAK;
    let mut raw: RAWINPUT = std::mem::zeroed();
    let mut size = std::mem::size_of::<RAWINPUT>() as u32;
    let got = GetRawInputData(lparam as HRAWINPUT, RID_INPUT, &mut raw as *mut RAWINPUT as *mut _, &mut size, std::mem::size_of::<RAWINPUTHEADER>() as u32);
    if got == u32::MAX || got == 0 || raw.header.dwType != RIM_TYPEKEYBOARD {
        return false;
    }
    raw.data.keyboard.Flags as u32 & RI_KEY_BREAK == 0
}

unsafe extern "system" fn find_window(hwnd: HWND, found: LPARAM) -> BOOL {
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid == std::process::id() && IsWindowVisible(hwnd) != 0 {
        *(found as *mut HWND) = hwnd;
        return 0;
    }
    TRUE
}

/// Hooks the game window once it exists (called every UI frame; retries every ~2 s).
pub fn ensure() {
    static TRIES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    if WINDOW.load(Ordering::Relaxed) != 0 || TRIES.fetch_add(1, Ordering::Relaxed) % 120 != 0 {
        return;
    }
    if let Err(e) = install() {
        log!("input: {e}");
    }
}

/// Subclasses the game window.
fn install() -> Result<(), String> {
    unsafe {
        let mut hwnd: HWND = std::ptr::null_mut();
        EnumWindows(Some(find_window), &mut hwnd as *mut HWND as LPARAM);
        if hwnd.is_null() {
            return Err("game window not found".into());
        }
        let original = GetWindowLongPtrW(hwnd, GWLP_WNDPROC);
        ORIGINAL.store(original, Ordering::Relaxed);
        WINDOW.store(hwnd as isize, Ordering::Relaxed);
        SetWindowLongPtrW(hwnd, GWLP_WNDPROC, wndproc as *const () as usize as isize);
        log!("input: text fields hooked into the game window");
    }
    Ok(())
}

/// Restores the game's window procedure (hot reload).
pub fn uninstall() {
    let hwnd = WINDOW.swap(0, Ordering::Relaxed);
    let original = ORIGINAL.load(Ordering::Relaxed);
    if hwnd != 0 && original != 0 {
        unsafe {
            SetWindowLongPtrW(hwnd as HWND, GWLP_WNDPROC, original);
        }
    }
    FOCUSED.store(false, Ordering::Relaxed);
}
