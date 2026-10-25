# UI and building blocks

Everything the addon draws comes from messages in `shroudlink_sdk::ui` (crate `shroudlink`,
`link/src/ui.rs`). Send them with `host.send(client, &msg.to_message())`. The addon draws
them with the game's own widgets.

## MODS tab windows

```rust
// an entry in the MODS tab list (send on join)
ToClient::Entries { owner: "my_mod".into(), entries: vec![MenuEntry { id: "open".into(), label: "My mod".into() }] }
// a window, shown on the right while the player has your entry selected; same id replaces it
ToClient::Open { owner: "my_mod".into(), window: Window { id: "main".into(), title: "My mod".into(), content: nodes } }
ToClient::Close { owner: "my_mod".into(), id: "main".into() }
```

**Nodes**

| Node | Event |
|---|---|
| `Text { text }`, `Heading { text }`, `Separator` | |
| `Row { children }` | |
| `Button { id, label }` | `Click` |
| `Toggle { id, label, value }` (an ON / OFF switch) | `Change { value: bool }` |
| `TextInput { id, value, hint }` | `Change { value }` on every edit (addon 0.6+), `Submit { value }` on Enter |
| `List { id, entries }` (flows into columns of 6) | `Click { index }` |
| `ItemGrid { id, columns, items: [Item { id, count }] }` (real item slots with icons and tooltips) | `Click { index }` |

Events arrive in `on_message`:

```rust
if let Some(UiEvent { window, node, event }) = UiEvent::from_message(message) {
    match (node.as_str(), event) {
        ("open", Event::Menu) => { /* the player chose your MODS entry */ }
        ("save", Event::Click { .. }) => { … }
        (_, Event::Closed) => { … }
        _ => {}
    }
}
```

Windows are redrawn from your last `Open`. To update one, send `Open` again with the
same id.

## Inventory building blocks

They work on the player's **backpack** slots, on menu screens (chest screen and inventory).

| Server → addon | |
|---|---|
| `ToClient::SlotKeys { owner, keys: [SlotKey { key: "L", label }] }` | a letter that acts on the hovered backpack slot |
| `ToClient::Marks { owner, marks: [Mark { slots, style: Rim \| Dim }] }` | gold rim or faded drawing; replaces your previous marks |
| `ToClient::Store { owner, data }` | keeps JSON on the player's PC for this server |
| `ToClient::Section { owner, id, nodes }` | rows in the chest screen's backpack panel, below its title; no nodes removes them; events come as `ui_event` with `window = id` |
| `ToClient::Notify { owner, text }` | a framed message for 8 s (at most 3 at a time, repeats counted) |
| `ToClient::Hud { owner, id, title, rows: [HudRow { label, value, fraction }] }` | a small see-through panel on the HUD (bottom right, also while playing); a gold bar under each row filled to `fraction` (0..1, 0: none); players drag it while a menu is open; no rows removes it. Send only changes |

| Addon → server (`FromClient::from_message(message)`) | |
|---|---|
| `SlotKey { key, slot, position, item }` | your key was pressed over a slot |
| `Stored { data }` | sent after every join if you stored something |
| `Layout { pages }` | the backpack's page entities in display order; sent when it changes |

**Positions vs. slot ids.** Entity ids change every session. A `position` is
`(page index in display order, slot 0..8)` and stays valid, so store positions. Map
them to this session's slot id with the layout:

```rust
let slot = layout[position.0 as usize] as u64 | (position.1 as u64) << 32;
```

The addon learns the layout when the player opens a chest screen.

**Example: slot locks in chestsort** (`mods/chestsort/src/locks.rs`)

1. On join, send `SlotKeys [L]`.
2. On `Stored`, read the positions. On `Layout`, remember the pages.
3. On `SlotKey L`, toggle the position, `Store` the positions, and send `Marks` with a
   rim on every locked slot.
4. In `keep`, return the locked slot ids.

## Compatibility

Messages an addon does not know are logged and ignored. A server mod should still work
(minus the UI) for players with an older addon or without one.
