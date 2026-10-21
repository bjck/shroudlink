//! Declarative UI that server mods send to client addons.
//!
//! The server describes a window as a tree of nodes; the addon draws it every
//! frame with the game's own widgets and reports interactions back to the mod
//! that owns the window. New server mods therefore need no client update.
//!
//! server -> client  `{"mod":"ui","op":"open","owner":"chestsort","window":{...}}`
//!                   `{"mod":"ui","op":"close","owner":"chestsort","id":"search"}`
//!                   `{"mod":"ui","op":"entries","owner":"chestsort","entries":[...]}`
//! client -> server  `{"mod":"chestsort","op":"ui_event","window":"search","node":"query","event":{...}}`

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Window {
    pub id: String,
    pub title: String,
    pub content: Vec<Node>,
}

/// An item as the game identifies it (`keen::ItemStack`).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Item {
    pub id: u32,
    pub count: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Node {
    Text { text: String },
    Heading { text: String },
    Separator,
    Row { children: Vec<Node> },
    Button { id: String, label: String },
    /// Single line text field; `submit` fires on Enter, `change` on edits.
    TextInput { id: String, value: String, #[serde(default)] hint: String },
    Toggle { id: String, label: String, value: bool },
    /// Item slots drawn like inventory slots (icon, count, tooltip); `click` reports the index.
    ItemGrid { id: String, columns: u8, items: Vec<Item> },
    /// Selectable text rows; `click` reports the index.
    List { id: String, entries: Vec<String> },
}

/// An entry the addon offers in its mod menu (opened with the addon hotkey).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct MenuEntry {
    pub id: String,
    pub label: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ToClient {
    Open { owner: String, window: Window },
    Close { owner: String, id: String },
    Entries { owner: String, entries: Vec<MenuEntry> },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    Click { #[serde(default)] index: Option<u32> },
    Change { value: serde_json::Value },
    Submit { value: String },
    /// The player chose a mod menu entry.
    Menu,
    /// The player closed the window.
    Closed,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct UiEvent {
    pub window: String,
    pub node: String,
    pub event: Event,
}

impl ToClient {
    /// The JSON message for the addon (`"mod":"ui"` routes it to the renderer).
    pub fn to_message(&self) -> serde_json::Value {
        let mut v = serde_json::to_value(self).expect("ui messages serialise");
        v["mod"] = "ui".into();
        v
    }
}

impl UiEvent {
    pub fn to_message(&self, owner: &str) -> serde_json::Value {
        serde_json::json!({ "mod": owner, "op": "ui_event", "event": self })
    }

    pub fn from_message(message: &serde_json::Value) -> Option<UiEvent> {
        (message.get("op")?.as_str()? == "ui_event").then(|| serde_json::from_value(message.get("event")?.clone()).ok())?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_json_shape() {
        let open = ToClient::Open {
            owner: "chestsort".into(),
            window: Window {
                id: "search".into(),
                title: "Search chests".into(),
                content: vec![
                    Node::TextInput { id: "query".into(), value: String::new(), hint: "item name".into() },
                    Node::ItemGrid { id: "results".into(), columns: 8, items: vec![Item { id: 7, count: 3 }] },
                ],
            },
        };
        let msg = open.to_message();
        assert_eq!(msg["mod"], "ui");
        assert_eq!(msg["op"], "open");
        assert_eq!(msg["window"]["content"][1]["type"], "item_grid");
        let mut back = msg.clone();
        back.as_object_mut().unwrap().remove("mod");
        assert_eq!(serde_json::from_value::<ToClient>(back).unwrap(), open);
    }

    #[test]
    fn event_roundtrip() {
        let e = UiEvent { window: "search".into(), node: "query".into(), event: Event::Submit { value: "iron".into() } };
        let msg = e.to_message("chestsort");
        assert_eq!(msg["mod"], "chestsort");
        assert_eq!(UiEvent::from_message(&msg), Some(e));
    }
}
