//! chestsort as a shroudlink mod (`shroudlink_mods/chestsort.dll`).
//!
//! - Quick stacking into a chest renamed to category words ("Food, Wood") first moves
//!   every stack of those categories into it, then the game's own quick stack runs.
//! - Slots locked with the client addon (L) never leave the player in any transfer.
//! - The MODS tab window lists the categories and finds the chest an item belongs in.

mod categories;
mod locks;
mod ui;

use std::collections::HashSet;

use shroudlink_sdk::{export_mod, log, Game, Host, InventoryEvent, Mod, TransferKind, Value};

pub(crate) const ID: &str = "chestsort";

#[derive(Default)]
pub struct Chestsort {
    ui: ui::Ui,
    locks: locks::Locks,
}

impl Mod for Chestsort {
    fn id(&self) -> &'static str {
        ID
    }

    fn on_load(&mut self) {
        categories::init();
    }

    fn on_join(&mut self, host: &mut Host, client: u64) {
        self.ui.on_join(host, client);
        self.locks.on_join(host, client);
    }

    fn on_message(&mut self, host: &mut Host, client: u64, message: &Value) {
        if !self.locks.on_message(host, client, message) {
            self.ui.on_message(host, client, message);
        }
    }

    fn on_leave(&mut self, _host: &mut Host, client: u64) {
        self.locks.forget(client);
        self.ui.on_leave(client);
    }

    fn keep(&mut self, game: &Game, event: &InventoryEvent) -> Vec<u64> {
        // the quick stack also runs from chest to player; only a player's own slots count
        if !self.locks.any() || !game.is_player(event.from) {
            return Vec::new();
        }
        let pages: HashSet<u32> = game.slots(event.from).iter().map(|s| s.page()).collect();
        let locked = self.locks.locked_in(&pages);
        log!("player {}: {} locked slot(s) kept", event.from, locked.len());
        locked
    }

    fn on_transfer(&mut self, game: &Game, event: &InventoryEvent) -> u32 {
        if event.kind != TransferKind::QuickStack || !game.is_player(event.from) {
            return 0;
        }
        let Some(name) = game.chest_name(event.to) else { return 0 };
        let items = categories::items_for(&name);
        if items.is_empty() {
            log!("chest {} '{name}': no category in name, vanilla only", event.to);
            return 0;
        }
        // unlike vanilla quick stacking, items with their own entity data (gear, weapon
        // gems) are moved too: the transfer carries them along with the slot
        let moved: u32 = game
            .slots(event.from)
            .iter()
            .filter(|s| s.count != 0 && items.contains(&s.item))
            .map(|s| game.move_stack(s.id, event.to))
            .sum();
        log!("chest {} '{name}' {:?}: moved {moved} item(s) by category", event.to, categories::parse(&name));
        moved
    }
}

export_mod!(Chestsort);

#[cfg(test)]
mod tests {
    use super::*;
    use shroudlink_sdk::testing::MockHost;
    use shroudlink_sdk::ui::{Event, UiEvent};

    fn click(m: &mut Chestsort, host: &mut MockHost, event: UiEvent) -> Vec<Value> {
        m.on_message(&mut host.host(), 5, &event.to_message(ID));
        host.take_sent().into_iter().map(|(_, v)| v).collect()
    }

    #[test]
    fn category_browser() {
        let mut m = Chestsort::default();
        let mut host = MockHost::default();
        m.on_join(&mut host.host(), 5);
        let joined = host.take_sent();
        let entries = joined.iter().map(|(_, v)| v).find(|v| v["mod"] == "ui" && v["op"] == "entries").expect("menu entry");
        assert_eq!(entries["entries"][0]["id"], "categories");

        let open = click(&mut m, &mut host, UiEvent { window: String::new(), node: "categories".into(), event: Event::Menu });
        let window = &open[0]["window"];
        assert_eq!(window["id"], "categories");
        let list = window["content"].as_array().unwrap().iter().find(|n| n["type"] == "list").unwrap();
        assert!(list["entries"].as_array().unwrap().len() >= 10);

        let open = click(&mut m, &mut host, UiEvent { window: "categories".into(), node: "category".into(), event: Event::Click { index: Some(0) } });
        let content = open[0]["window"]["content"].as_array().unwrap();
        let grid = content.iter().find(|n| n["type"] == "item_grid").expect("item grid for the category");
        assert!(grid["items"][0]["id"].as_u64().unwrap() > 0);

        // item search through the text field
        let open = click(&mut m, &mut host, UiEvent { window: "categories".into(), node: "search".into(), event: Event::Submit { value: "iron bar".into() } });
        let content = open[0]["window"]["content"].as_array().unwrap();
        assert_eq!(content.iter().find(|n| n["type"] == "text_input").unwrap()["value"], "iron bar");
        assert!(content.iter().any(|n| n["id"] == "found:Ores"), "iron bar listed under Ores");

        // paging: every Next click advances one page
        click(&mut m, &mut host, UiEvent { window: "categories".into(), node: "category".into(), event: Event::Click { index: Some(7) } });
        for page in 2..=4 {
            let open = click(&mut m, &mut host, UiEvent { window: "categories".into(), node: "next".into(), event: Event::Click { index: None } });
            assert!(open[0]["window"].to_string().contains(&format!("Page {page} /")), "after Next: {}", open[0]["window"]);
        }
    }

}
