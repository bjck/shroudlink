//! Keyboard input for mod text fields.
//!
//! The game window's message procedure is subclassed. While a mod text field has
//! focus, typed characters and editing keys go to that field and are kept away
//! from the game (otherwise typing "j" would switch to the journal tab).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU8, Ordering};
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
/// While a search field is on screen, typing "/" jumps into it.
static SEARCH_HOTKEY: AtomicBool = AtomicBool::new(false);
static SEARCH_REQUESTED: AtomicBool = AtomicBool::new(false);

/// "L" press for the hovered backpack slot: 0 none, 1 pressed, 2 pressed and one UI
/// frame has ended since. A press can arrive between the slots being drawn and the
/// end of the frame, so it stays valid through the whole next frame.
static LOCK_KEY: AtomicU8 = AtomicU8::new(0);

/// True (once) if L was pressed while the cursor is inside `rect` (x, y, w, h, UI units).
pub fn lock_key_pressed_over(rect: [f32; 4], (x, y): (f32, f32)) -> bool {
    if LOCK_KEY.load(Ordering::Relaxed) == 0 {
        return false;
    }
    let inside = x >= rect[0] && x < rect[0] + rect[2] && y >= rect[1] && y < rect[1] + rect[3];
    inside && LOCK_KEY.swap(0, Ordering::Relaxed) != 0
}

/// End of a UI frame: forgets an L press that a whole frame of slots did not take.
pub fn clear_lock_key() {
    let _ = LOCK_KEY.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |s| Some(if s == 1 { 2 } else { 0 }));
}

/// Set on menu screens, where L can lock the hovered backpack slot.
static LOCK_ARMED: AtomicBool = AtomicBool::new(false);

pub fn arm_lock_key(armed: bool) {
    LOCK_ARMED.store(armed, Ordering::Relaxed);
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
    if !FOCUSED.load(Ordering::Relaxed) && SEARCH_HOTKEY.load(Ordering::Relaxed) && msg == WM_CHAR && wparam == '/' as usize {
        SEARCH_REQUESTED.store(true, Ordering::Relaxed);
        return 0;
    }
    if !FOCUSED.load(Ordering::Relaxed) && LOCK_ARMED.load(Ordering::Relaxed) && msg == WM_KEYDOWN && wparam == b'L' as usize && lparam & (1 << 30) == 0 {
        // a fresh press (not auto-repeat); the slot under the cursor takes it this frame
        LOCK_KEY.store(1, Ordering::Relaxed);
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
