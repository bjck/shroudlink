//! chestsort's in-game UI over shroudlink: browse the sorting categories, the
//! chest names that select them and their items (shown as real item slots).

use std::collections::HashMap;

use shroudlink_sdk::ui::{Event, Item, MenuEntry, Node, ToClient, UiEvent, Window};
use shroudlink_sdk::{Host, Value};

use crate::{categories, ID};
const MENU_CATEGORIES: &str = "categories";
const WINDOW: &str = "categories";
/// Item slots shown per category page.
const PAGE: usize = 32;

#[derive(Default)]
pub struct Ui {
    /// selected category per client
    selected: HashMap<u64, (String, usize)>,
    /// last item search per client
    searches: HashMap<u64, String>,
}

/// Search results shown at most.
const SEARCH_LIMIT: usize = 48;

impl Ui {
    fn window(&self, client: u64) -> Window {
        let overview = categories::overview();
        let names: Vec<String> = overview.iter().map(|(n, _, ids)| format!("{n} ({})", ids.len())).collect();
        let query = self.searches.get(&client).cloned().unwrap_or_default();
        let mut content = vec![
            Node::Text { text: "Name a chest with category words to fill it when you quick stack.".into() },
            Node::TextInput { id: "search".into(), value: query.clone(), hint: "Search items, e.g. iron bar".into() },
        ];
        if !query.is_empty() {
            // the point of the search: which chest (category) an item goes into
            let found = categories::search(&query, SEARCH_LIMIT);
            content.push(Node::Text { text: format!("{} item(s) match \"{query}\"", found.len()) });
            let mut unsorted: Vec<u32> = found.clone();
            for (name, words, ids) in &overview {
                let matching: Vec<u32> = found.iter().copied().filter(|id| ids.binary_search(id).is_ok()).collect();
                if matching.is_empty() {
                    continue;
                }
                unsorted.retain(|id| !matching.contains(id));
                content.push(Node::Heading { text: format!("{name} · chest names: {}", words.join(", ")) });
                content.push(Node::ItemGrid { id: format!("found:{name}"), columns: 8, items: matching.iter().map(|&id| Item { id, count: 1 }).collect() });
            }
            if !unsorted.is_empty() {
                content.push(Node::Heading { text: "Not in any category (stays in your inventory)".into() });
                content.push(Node::ItemGrid { id: "found:none".into(), columns: 8, items: unsorted.iter().map(|&id| Item { id, count: 1 }).collect() });
            }
            content.push(Node::Separator);
        }
        content.push(Node::List { id: "category".into(), entries: names });
        if let Some((name, page)) = self.selected.get(&client) {
            if let Some((_, words, ids)) = overview.iter().find(|(n, _, _)| n == name) {
                let pages = ids.len().div_ceil(PAGE).max(1);
                let page = (*page).min(pages - 1);
                content.push(Node::Separator);
                content.push(Node::Heading { text: name.clone() });
                content.push(Node::Text { text: format!("Chest names: {}", words.join(", ")) });
                content.push(Node::ItemGrid {
                    id: "items".into(),
                    columns: 8,
                    items: ids.iter().skip(page * PAGE).take(PAGE).map(|&id| Item { id, count: 1 }).collect(),
                });
                if pages > 1 {
                    content.push(Node::Row {
                        children: vec![
                            Node::Button { id: "prev".into(), label: "Previous".into() },
                            Node::Text { text: format!("Page {} / {pages}", page + 1) },
                            Node::Button { id: "next".into(), label: "Next".into() },
                        ],
                    });
                }
            }
        }
        Window { id: WINDOW.into(), title: "Chest categories".into(), content }
    }

    fn show(&self, host: &mut Host, client: u64) {
        let open = ToClient::Open { owner: ID.into(), window: self.window(client) };
        host.send(client, &open.to_message());
    }
}

impl Ui {
    pub fn on_join(&mut self, host: &mut Host, client: u64) {
        let entries = ToClient::Entries {
            owner: ID.into(),
            entries: vec![MenuEntry { id: MENU_CATEGORIES.into(), label: "Chest categories".into() }],
        };
        host.send(client, &entries.to_message());
    }

    pub fn on_message(&mut self, host: &mut Host, client: u64, message: &Value) {
        let Some(UiEvent { node, event, .. }) = UiEvent::from_message(message) else { return };
        match (node.as_str(), event) {
            (MENU_CATEGORIES, Event::Menu) => self.show(host, client),
            ("category", Event::Click { index: Some(i) }) => {
                if let Some((name, _, _)) = categories::overview().get(i as usize) {
                    self.selected.insert(client, (name.clone(), 0));
                }
                self.show(host, client);
            }
            ("search", Event::Submit { value }) => {
                self.searches.insert(client, value.trim().to_owned());
                self.show(host, client);
            }
            ("next" | "prev", Event::Click { .. }) => {
                if let Some((_, page)) = self.selected.get_mut(&client) {
                    *page = if node == "next" { *page + 1 } else { page.saturating_sub(1) };
                }
                self.show(host, client);
            }
            (_, Event::Closed) => {
                self.selected.remove(&client);
                self.searches.remove(&client);
            }
            _ => {}
        }
    }

    pub fn on_leave(&mut self, client: u64) {
        self.selected.remove(&client);
        self.searches.remove(&client);
    }
}
