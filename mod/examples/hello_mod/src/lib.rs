//! Example shroudlink mod DLL: a menu entry that opens a window with a counter, a "Wave"
//! button in the chest screen's backpack panel that shows a HUD message, and game events:
//! deaths and pickups are counted, level-ups and deaths announced to everyone.
//! Build it, drop `hello_mod.dll` into `shroudlink_mods/` next to the server.

use std::collections::HashMap;

use shroudlink_sdk::ui::{Event, MenuEntry, Node, ToClient, UiEvent, Window};

/// The row hello adds to the chest screen's backpack panel.
const SECTION: &str = "wave";
use shroudlink_sdk::{export_mod, Events, GameEvent, Host, Known, Mod, Value};

#[derive(Default)]
pub struct Hello {
    clicks: HashMap<u64, u32>,
    shout: HashMap<u64, bool>,
    /// connected addon sessions, to announce game events to
    clients: Vec<u64>,
    deaths: u32,
    pickups: u32,
}

impl Hello {
    fn window(&self, client: u64) -> ToClient {
        let clicks = self.clicks.get(&client).copied().unwrap_or(0);
        let shout = self.shout.get(&client).copied().unwrap_or(false);
        let greeting = if shout { "HELLO FROM A MOD DLL!" } else { "This window comes from hello_mod.dll on the server." };
        ToClient::Open {
            owner: "hello".into(),
            window: Window {
                id: "hello".into(),
                title: "Hello from a mod DLL".into(),
                content: vec![
                    Node::Text { text: greeting.into() },
                    Node::Text { text: format!("Seen on this server: {} death(s), {} pickup(s)", self.deaths, self.pickups) },
                    Node::Toggle { id: "shout".into(), label: "Shout".into(), value: shout },
                    Node::Row {
                        children: vec![
                            Node::Button { id: "click".into(), label: "Click me".into() },
                            Node::Text { text: format!("clicked {clicks} time(s)") },
                        ],
                    },
                ],
            },
        }
    }
}

impl Mod for Hello {
    fn id(&self) -> &'static str {
        "hello"
    }

    fn game_events(&self) -> Vec<&'static str> {
        vec!["EntityDiedEvent", "LevelUpEvent", "PickupEvent"]
    }

    fn on_game_event(&mut self, events: &Events, event: &GameEvent) {
        let text = match event.known() {
            Some(Known::EntityDied { entity }) => {
                self.deaths += 1;
                format!("Entity {entity} died ({} so far)", self.deaths)
            }
            Some(Known::LevelUp { player, level }) => format!("Player {player} reached level {level}!"),
            Some(Known::Pickup { .. }) => {
                self.pickups += 1;
                return;
            }
            _ => return,
        };
        for &client in &self.clients {
            events.send(client, &ToClient::Notify { owner: "hello".into(), text: text.clone() }.to_message());
        }
    }

    fn on_join(&mut self, host: &mut Host, client: u64) {
        self.clients.push(client);
        let entries = ToClient::Entries { owner: "hello".into(), entries: vec![MenuEntry { id: "open".into(), label: "Hello mod".into() }] };
        host.send(client, &entries.to_message());
        // a row in every chest screen's backpack panel
        let section = ToClient::Section { owner: "hello".into(), id: SECTION.into(), nodes: vec![Node::Button { id: "wave".into(), label: "Wave".into() }] };
        host.send(client, &section.to_message());
    }

    fn on_message(&mut self, host: &mut Host, client: u64, message: &Value) {
        let Some(UiEvent { window, node, event }) = UiEvent::from_message(message) else { return };
        if window == SECTION {
            let note = ToClient::Notify { owner: "hello".into(), text: "Hello from hello_mod!".into() };
            host.send(client, &note.to_message());
            return;
        }
        match (node.as_str(), event) {
            ("open", Event::Menu) => {}
            ("click", Event::Click { .. }) => *self.clicks.entry(client).or_default() += 1,
            ("shout", Event::Change { value }) => {
                self.shout.insert(client, value.as_bool().unwrap_or(false));
            }
            _ => return,
        }
        host.send(client, &self.window(client).to_message());
    }

    fn on_leave(&mut self, _host: &mut Host, client: u64) {
        self.clients.retain(|&c| c != client);
        self.clicks.remove(&client);
        self.shout.remove(&client);
    }
}

export_mod!(Hello);
