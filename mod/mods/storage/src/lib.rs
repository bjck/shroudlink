//! Storage: one window with everything stored in the chests around the player, as if it
//! were one big box. Clicking an item fetches its stacks from the chests into the backpack.
//! Built only from the SDK's world access (`player_of`, `inventories_near`, `inventory`,
//! `move_stack`); the server keeps only the world around players loaded, so "around" is
//! what counts.

use std::collections::{BTreeMap, HashMap};

use shroudlink_sdk::ui::{Event, Item, MenuEntry, Node, ToClient, UiEvent, Window};
use shroudlink_sdk::{export_mod, log, Host, Mod, Value, World};

const ID: &str = "storage";
/// How far around the player chests count, in metres.
const RADIUS: f64 = 40.0;
/// Item slots shown at most.
const SHOWN: usize = 64;

/// Everything stored around a player: item -> (total, the stacks holding it).
#[derive(Default)]
struct Contents {
    chests: usize,
    items: BTreeMap<u32, (u32, Vec<(u32, u64)>)>,
}

fn contents(world: &World, player: u32) -> Contents {
    let mut out = Contents::default();
    for chest in world.inventories_near(player, RADIUS) {
        out.chests += 1;
        for slot in world.inventory(chest).into_iter().filter(|s| s.count != 0) {
            let entry = out.items.entry(slot.item).or_default();
            entry.0 += slot.count;
            entry.1.push((chest, slot.id));
        }
    }
    out
}

/// The items shown, most first (the grid's click index points into this).
fn shown(contents: &Contents) -> Vec<(u32, u32)> {
    let mut items: Vec<(u32, u32)> = contents.items.iter().map(|(&item, &(total, _))| (item, total)).collect();
    items.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    items.truncate(SHOWN);
    items
}

#[derive(Default)]
pub struct Storage {
    /// what each client's window shows, for its click indices
    showing: HashMap<u64, Vec<(u32, u32)>>,
    status: HashMap<u64, String>,
}

impl Storage {
    fn window(&mut self, host: &Host, client: u64) -> ToClient {
        let world = host.world();
        let mut content = Vec::new();
        match world.player_of(client) {
            None => content.push(Node::Text { text: "Open any chest once, so the server knows which player you are.".into() }),
            Some(player) => {
                let all = contents(&world, player);
                let items = shown(&all);
                content.push(Node::Text { text: format!("{} chest(s) within {RADIUS:.0} m, {} kind(s) of items. Click an item to take it.", all.chests, all.items.len()) });
                content.push(Node::ItemGrid { id: "items".into(), columns: 8, items: items.iter().map(|&(id, count)| Item { id, count }).collect() });
                self.showing.insert(client, items);
            }
        }
        if let Some(status) = self.status.get(&client) {
            content.push(Node::Text { text: status.clone() });
        }
        content.push(Node::Button { id: "refresh".into(), label: "Refresh".into() });
        ToClient::Open { owner: ID.into(), window: Window { id: "main".into(), title: "Storage".into(), content } }
    }

    /// Moves every stack of `item` from the chests around the player into the backpack.
    fn take(&mut self, host: &Host, client: u64, item: u32) -> String {
        let world = host.world();
        let Some(player) = world.player_of(client) else { return "Unknown player: open a chest first.".into() };
        let all = contents(&world, player);
        let Some((total, stacks)) = all.items.get(&item) else { return "That item is gone.".into() };
        let mut moved = 0;
        for &(chest, slot) in stacks {
            moved += world.move_stack(chest, slot, player);
        }
        log!("storage: player {player} took {moved} of {total} x item {item}");
        if moved < *total {
            format!("Took {moved} of {total} - the backpack is full.")
        } else {
            format!("Took {moved}.")
        }
    }
}

impl Mod for Storage {
    fn id(&self) -> &'static str {
        ID
    }

    fn on_join(&mut self, host: &mut Host, client: u64) {
        let entries = ToClient::Entries { owner: ID.into(), entries: vec![MenuEntry { id: "open".into(), label: "Storage".into() }] };
        host.send(client, &entries.to_message());
    }

    fn on_message(&mut self, host: &mut Host, client: u64, message: &Value) {
        let Some(UiEvent { node, event, .. }) = UiEvent::from_message(message) else { return };
        match (node.as_str(), event) {
            ("open", Event::Menu) | ("refresh", Event::Click { .. }) => {}
            ("items", Event::Click { index: Some(i) }) => {
                let item = self.showing.get(&client).and_then(|s| s.get(i as usize)).map(|&(item, _)| item);
                if let Some(item) = item {
                    let status = self.take(host, client, item);
                    self.status.insert(client, status);
                }
            }
            _ => return,
        }
        let window = self.window(host, client);
        host.send(client, &window.to_message());
    }

    fn on_leave(&mut self, _host: &mut Host, client: u64) {
        self.showing.remove(&client);
        self.status.remove(&client);
    }
}

export_mod!(Storage);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_are_shown_most_first() {
        let mut c = Contents::default();
        c.items.insert(5, (3, vec![(100, 1)]));
        c.items.insert(7, (40, vec![(100, 2), (101, 3)]));
        c.items.insert(6, (40, vec![(101, 4)]));
        assert_eq!(shown(&c), vec![(6, 40), (7, 40), (5, 3)]);
    }
}
