# shroudlink: server mods with in-game UI for Enshrouded

shroudlink lets server mods talk to players and show windows in the game's own UI style.
All mod logic runs on the server; players install one small, generic client addon once.

```
 server (enshrouded_server.exe)                      player (enshrouded.exe)
 ┌───────────────────────────────┐                  ┌─────────────────────────────┐
 │ steam_api64.dll  (host)       │  game netcode    │ dinput8.dll  (client addon) │
 │  ├─ built-in mods (chestsort) │◀────────────────▶│  ├─ channel                 │
 │  └─ shroudlink_mods/*.dll     │ custom-string    │  └─ native UI renderer      │
 └───────────────────────────────┘ messages         └─────────────────────────────┘
```

## How the channel works
The game replicates "custom player strings" (chest names etc.) both ways: clients send
`CustomPlayerStringClientMessage` (rename), the server sends `CustomPlayerStringServerMessage`
per player. shroudlink packets are such strings with the prefix `~sl1~` (base64url frames,
≤ 256 chars); host and addon take them out before the game stores them, so nothing reaches
name tables or savegames.

1. The host sends a beacon (`WhoAmI{nonce 0, slot}`) to every player slot without a session
   (repeated every 30 s; the text never changes, so vanilla clients store it once).
2. The addon answers with `Claim{nonce, slot}`. It never sends before a beacon, so vanilla
   servers never see a packet.
3. Both sides exchange JSON messages, chunked into `Data` frames: `{"mod": "<id>", ...}`.

## Writing a mod
Build a DLL with the `shroudlink-sdk` crate and drop it into `shroudlink_mods/` next to
`enshrouded_server.exe` (loaded at server start). See `examples/hello_mod`:

```rust
use shroudlink_sdk::ui::{Event, MenuEntry, Node, ToClient, UiEvent, Window};
use shroudlink_sdk::{export_mod, Host, Mod, Value};

#[derive(Default)]
pub struct Hello;

impl Mod for Hello {
    fn id(&self) -> &'static str { "hello" }

    // a player with the addon connected: add an entry to their mod menu
    fn on_join(&mut self, host: &mut Host, client: u64) {
        let entries = ToClient::Entries { owner: "hello".into(),
            entries: vec![MenuEntry { id: "open".into(), label: "Hello mod".into() }] };
        host.send(client, &entries.to_message());
    }

    // UI events (menu choice, clicks, ...) and any other message addressed to "hello"
    fn on_message(&mut self, host: &mut Host, client: u64, message: &Value) {
        if let Some(UiEvent { event: Event::Menu, .. }) = UiEvent::from_message(message) {
            let window = Window { id: "hello".into(), title: "Hello".into(),
                content: vec![Node::Text { text: "Hi!".into() }] };
            host.send(client, &ToClient::Open { owner: "hello".into(), window }.to_message());
        }
    }
}

export_mod!(Hello);
```

Build: `cargo build --release -p hello_mod` → `target/release/hello_mod.dll`.

The C ABI (`shroudlink_mod_v1`, `HostApi`, `ModDecl` in `sdk/src/lib.rs`) passes JSON text,
so mods can be written in any language that can export a C function.

## UI nodes
Windows are trees of nodes (`link/src/ui.rs`), drawn with the game's widgets:

| node | shows | events |
|---|---|---|
| `text`, `heading` | game font, body / title style | – |
| `separator` | vertical space | – |
| `row` | children side by side | – |
| `button` | clickable label | `click` |
| `toggle` | [x] / [ ] label | `change {value}` |
| `list` | clickable rows | `click {index}` |
| `item_grid` | real inventory slots (icon, rarity, tooltip) | `click {index}` |
| `text_input` | framed field; click to type (keyboard is kept from the game) | `submit {value}` on Enter |

Messages: server → client `{"mod":"ui","op":"open"|"close"|"entries", "owner": ...}`;
client → server `{"mod":"<owner>","op":"ui_event","event":{"window","node","event"}}`.
Mods live on their own **MODS tab** in the player menu (next to SKILLS): mod entries on the
left, the selected mod's windows on the right. Windows opened while another screen with a
cursor is shown (e.g. a chest) appear as panels on the right.

## Built-in client features
- **Item search on the chest screen:** "SEARCH ( / )" in the backpack panel header. Click it
  or press `/`, type: slots of both panels whose (localised) item name does not match turn
  grey while typing. `Esc` clears, a second `Esc` leaves the field, `X` clears.

## Status (Oct 2026, game build 25988414)
- Channel: verified end to end in game (beacon, claim, messages both ways, chunking).
- Native UI, verified in game: MODS tab in the player menu, page with mod list, text,
  headings, lists (multi-column), buttons and paging, item grids with real item icons and
  clicks, text fields (typing, Enter, Esc) with the keyboard kept from the game.
- Not clicked in game yet: toggles.
- Host: unit tests cover sessions, chunking, reconnects, the chestsort UI mod (incl. paging)
  and loading `hello_mod.dll` through the C ABI (`cargo test -p chestsort-server`).

## Lessons from the game's UI (keen UI)
- Frames are identified by a 64-bit key; siblings need unique keys, and a key must stay with
  the same widget across frames (derive it from the content, not a counter).
- A frame without a size fills its parent; give panels explicit sizes.
- Layers are identified by kind: one layer per kind and frame (popups: 0x1d).
- Mod buttons test the cursor against their layout rectangle (`node+0x114`, UI units of a
  1920x1080 reference) instead of the game's interaction state, which misfires when a window
  is replaced from outside.

## Files
| path | role |
|---|---|
| `link/` | protocol + UI schema (shared) |
| `server/` | host (`steam_api64.dll` proxy) incl. chestsort and the plugin loader |
| `client/` | client addon (`dinput8.dll`) |
| `sdk/` | mod SDK (C ABI + Rust trait) |
| `examples/hello_mod/` | example mod DLL |
