//! Test automation: with a `shroudlink_automation` file in the game folder, the addon
//! answers state queries on 127.0.0.1:47911 (one JSON object per line), so a test script
//! (research/scripts/gamectl.py) can see what is on screen while it drives the game with
//! real mouse and keyboard input. Nothing listens without the file, and only locally.
//!
//! `{"cmd":"state"}` -> screen, connection, mod menu and windows, backpack sections, HUD
//! messages, cursor, and every backpack slot drawn in the last frame with its rectangle.
//! Rectangles and the cursor are in UI units (1080 high); `client` is the window size in
//! pixels, so pixel = unit * client.height / 1080.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::Mutex;

use chestsort_common::log;
use serde_json::{json, Value};

const MARKER: &str = "shroudlink_automation";
const ADDRESS: &str = "127.0.0.1:47911";

/// Backpack slots drawn this frame and in the last complete one.
static SLOTS: Mutex<(Vec<Value>, Vec<Value>)> = Mutex::new((Vec::new(), Vec::new()));

/// Checked once at start: the slot hook asks every frame.
pub fn enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| chestsort_common::game_dir().join(MARKER).exists())
}

/// A backpack slot the game drew (called from the slot hook).
pub fn record_slot(slot: u64, position: (u32, u32), item: u32, rect: [f32; 4], mark: Option<&str>) {
    SLOTS.lock().unwrap().0.push(json!({ "slot": slot, "position": [position.0, position.1], "item": item, "rect": rect, "mark": mark }));
}

/// End of a UI frame: the slots drawn in it become the reported ones.
pub fn end_frame() {
    let mut slots = SLOTS.lock().unwrap();
    let (current, last) = &mut *slots;
    if !current.is_empty() || !last.is_empty() {
        *last = std::mem::take(current);
    }
}

fn state() -> Value {
    let mut state = crate::ui::snapshot();
    state["slots"] = Value::Array(SLOTS.lock().unwrap().1.clone());
    // the session survives a lost connection until a new one is made: the client only ticks
    // in a world, so a recent tick is what "in the world" means
    let tick = crate::link::since_last_tick();
    state["last_tick_ms"] = json!(tick);
    state["in_world"] = json!(crate::link::connected() && tick.is_some_and(|ms| ms < 2000));
    state["connected"] = crate::link::connected().into();
    state["server_mods"] = Value::Array(crate::link::server_mods());
    state["mouse"] = json!(crate::native::mouse_pos());
    state["client"] = json!(crate::native::client_size());
    state
}

fn answer(request: &str) -> Value {
    let request: Value = serde_json::from_str(request).unwrap_or(Value::Null);
    match request.get("cmd").and_then(Value::as_str) {
        Some("state") => state(),
        Some("ping") => json!({ "pong": true }),
        // a message to the server, as if a mod's UI had sent it; the reply is logged
        Some("send") => match request.get("message") {
            Some(message) => {
                crate::link::send(message.clone());
                json!({ "sent": true })
            }
            None => json!({ "error": "send needs a message" }),
        },
        // a message from the server, as if a mod had sent it (UI tests without a mod)
        Some("receive") => match request.get("message") {
            Some(message) => {
                crate::link::dispatch(message);
                json!({ "received": true })
            }
            None => json!({ "error": "receive needs a message" }),
        },
        Some("log") => {
            let n = request.get("lines").and_then(Value::as_u64).unwrap_or(20) as usize;
            let text = std::fs::read_to_string(chestsort_common::game_dir().join("shroudlink_client.log")).unwrap_or_default();
            let lines: Vec<&str> = text.lines().rev().take(n).collect();
            json!({ "lines": lines.into_iter().rev().collect::<Vec<_>>() })
        }
        _ => json!({ "error": "unknown command; use state, ping, send, receive or log" }),
    }
}

pub fn start() {
    if !enabled() {
        return;
    }
    std::thread::spawn(|| {
        let listener = match TcpListener::bind(ADDRESS) {
            Ok(l) => l,
            Err(e) => {
                log!("automation: cannot listen on {ADDRESS} ({e})");
                return;
            }
        };
        log!("automation: listening on {ADDRESS}");
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let mut writer = match stream.try_clone() {
                    Ok(w) => w,
                    Err(_) => return,
                };
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    let reply = answer(&line).to_string();
                    if writer.write_all(reply.as_bytes()).and_then(|()| writer.write_all(b"\n")).is_err() {
                        break;
                    }
                }
            });
        }
    });
}
