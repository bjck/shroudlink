//! Mods the player switched off in the MODS page list. The addon hides their UI (windows,
//! backpack panel rows, HUD panels and messages, slot marks) and ignores their slot keys;
//! everything they sent stays, so switching back on shows it again at once. Only this PC
//! is affected: the mods keep running on the server. Kept in `shroudlink_mods_off.json`.

use std::sync::Mutex;

use chestsort_common::log;

const FILE: &str = "shroudlink_mods_off.json";

static OFF: Mutex<Option<Vec<String>>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Vec<String>) -> R) -> R {
    let mut off = OFF.lock().unwrap();
    let off = off.get_or_insert_with(|| {
        std::fs::read_to_string(chestsort_common::game_dir().join(FILE)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    });
    f(off)
}

/// True unless the player switched the mod `owner` off.
pub fn is_on(owner: &str) -> bool {
    with(|off| !off.iter().any(|o| o == owner))
}

/// Switches `owner` on or off and keeps the choice.
pub fn toggle(owner: &str) {
    let on = with(|off| {
        let on = off.iter().any(|o| o == owner);
        if on {
            off.retain(|o| o != owner);
        } else {
            off.push(owner.to_owned());
        }
        let _ = std::fs::write(chestsort_common::game_dir().join(FILE), serde_json::to_string_pretty(off).unwrap_or_default());
        on
    });
    log!("switches: {owner} {}", if on { "on" } else { "off" });
    crate::slots::arm_keys();
}

/// The mods switched off, for test automation.
pub fn off() -> Vec<String> {
    with(|off| off.clone())
}
