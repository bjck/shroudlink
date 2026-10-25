# shroudlink: server mods with in-game UI for Enshrouded

shroudlink lets server mods talk to players and show windows in the game's own UI style.
All mod logic runs on the server; players install one small, generic client addon once.

```
 server (enshrouded_server.exe)                      player (enshrouded.exe)
 ┌───────────────────────────────┐                  ┌─────────────────────────────┐
 │ steam_api64.dll  (core)       │  game netcode    │ dinput8.dll  (client addon) │
 │  ├─ hooks + game API          │◀────────────────▶│  ├─ channel                 │
 │  └─ shroudlink_mods/*.dll     │ custom-string    │  └─ native UI renderer      │
 │      (chestsort, your mod)    │ messages         │                             │
 └───────────────────────────────┘                  └─────────────────────────────┘
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
Start with `python research/scripts/newmod.py my_mod`: it creates `mod/mods/my_mod/` with a
MODS tab window, inventory event stubs and a unit test, and adds it to the workspace.

Build a DLL with the `shroudlink-sdk` crate and drop it into `shroudlink_mods/` next to
`enshrouded_server.exe`. It is loaded at server start, and **reloaded within seconds
whenever you overwrite it** while players are connected (hot reload). The design is in
[`docs/sdk-v2.md`](docs/sdk-v2.md).

A mod can:
- talk to client addons and show windows in the MODS tab (`on_join`, `on_message`, `Host::send`);
- take part in every inventory transfer (quick stack, move all, Quick Stack Station):
  `keep` names slots that must stay, `on_transfer` moves stacks itself through `Game`
  (`slots`, `is_player`, `chest_name`, `max_stack`, `move_stack`, `send`).

- work with the player's backpack slots through the addon's **building blocks**
  (`shroudlink::ui`): `SlotKeys` (a letter that acts on the hovered slot → `FromClient::SlotKey`
  with slot id and a session-independent position), `Marks` (rim or dimmed slots), `Store`
  (JSON kept on the player's PC per server → `FromClient::Stored` on every join) and
  `FromClient::Layout` (page order, to map stored positions to this session's slot ids).
  The addon never needs an update for a new feature built from them; chestsort's locks are
  one (`mods/chestsort/src/locks.rs`, about 100 lines).
- add rows to the chest screen's backpack panel (`Section`, any UI nodes; events come back
  as `ui_event` with the section id as window) and show short HUD messages (`Notify`).
  `examples/hello_mod` adds a "Wave" button that answers with a message.
- show a small **HUD panel** (`Hud`: a title and rows of label, value and a gold bar filled
  to `fraction`), also while playing, bottom right and see-through; players drag it while
  a menu is open and the addon keeps where. Send it again when it changes, with no rows to
  remove it; older addons ignore it. The damage meter's fight panel is one (`mods/meter`).
- players can switch a mod off in their addon (the [X] in the MODS page list, since 0.5.0):
  the addon then hides that mod's windows, backpack rows, HUD panels and messages and slot
  marks, and ignores its slot keys. Your mod is not told and keeps running for that player,
  so keep the server side correct without the UI (chestsort's locks still protect slots).

- react to **game events**: `game_events()` names the types (e.g. "EntityDiedEvent",
  "PickupEvent", "LevelUpEvent"), and `on_game_event` gets each occurrence;
  `event.known()` decodes the common ones. `examples/hello_mod` announces deaths and
  level-ups.
- work with the **world** from any callback (`host.world()`): players, positions,
  teleport, any component, nearby inventories and entities with a component
  (`entities_near`), moving stacks or exact counts (`move_items`), creating and removing plain items
  (`give_items`, `remove_items`), and vanilla **buffs**
  (`apply_buff`, through the game's impact VM: `server/src/buffs.rs`). `on_tick` runs
  about once a second. `mods/travel`, `mods/storage`, `mods/lantern` and `mods/assistants`
  use them.

Equipped items, the item in hand and the actionbar are always protected by the core.
`mods/chestsort` is a complete real-world mod (categories, sorting, locks, UI with tests).

A minimal inventory mod:

```rust
use shroudlink_sdk::{export_mod, Game, InventoryEvent, Mod};

#[derive(Default)]
pub struct KeepTorches;

impl Mod for KeepTorches {
    fn id(&self) -> &'static str { "keep_torches" }
    // never quick stack or move torches out of a player's inventory
    fn keep(&mut self, game: &Game, event: &InventoryEvent) -> Vec<u64> {
        if !game.is_player(event.from) { return Vec::new(); }
        game.slots(event.from).iter().filter(|s| s.item == TORCH_ID).map(|s| s.id).collect()
    }
}

export_mod!(KeepTorches);
```

A mod with UI (see `examples/hello_mod`):

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

The C ABI (`shroudlink_mod_v2`, `HostApi`, `ModDecl` in `sdk/src/lib.rs`) passes JSON text
and plain structs, so mods can be written in any language that can export a C function.
v1 mods (`shroudlink_mod_v1`) still load; they get no inventory events.

Test a mod inside a real dedicated server without starting the game:
`python research/scripts/selftest.py` runs the core's self-test (fake addon session, quick
stack and move all through the game's own functions, hot reload) and restores the savegame
afterwards.

Unit-test a mod without the game with `shroudlink_sdk::testing::MockHost`, which records
what the mod sends (see the tests in `mods/chestsort/src/lib.rs`). Mod log lines
(`shroudlink_sdk::log!`) go to the server's `shroudlink.log`.

## UI nodes
Windows are trees of nodes (`link/src/ui.rs`), drawn with the game's widgets:

| node | shows | events |
|---|---|---|
| `text`, `heading` | game font, body / title style (text wraps; a heading gets a gold line) | – |
| `separator` | gold line | – |
| `row` | children side by side | – |
| `button` | framed button, lit under the cursor | `click` |
| `toggle` | ON / OFF switch and label | `change {value}` |
| `list` | clickable rows; tab-separated cells line up in columns | `click {index}` |
| `tabs` | a tab bar along the top (`tabs`, `selected`); send the selected tab's content after it | `click {index}` |
| `item_grid` | real inventory slots (icon, rarity) with the game's item tooltip | `click {index}` |
| `text_input` | framed field; click to type (keyboard is kept from the game) | `change {value}` on every edit (addon 0.6+), `submit {value}` on Enter |

Labels must not contain `<`: the game's text starts a markup tag there and draws nothing
after it (use « and »). A button next to a text field (Order) gets the typed text from the
`change` events, as `submit` only comes with Enter.

**UI protocol** (`shroudlink::ui::UI_PROTOCOL`, now 2 = addon and server 0.6): the
welcome carries it, the addon answers `{"mod":"shroudlink","op":"hello","version":…,"ui_protocol":…}`.
Version 2 broke older addons (`tabs` nodes, text-field `change`): an addon without `hello`
is told on screen to update, and a newer addon on an older server says so too.

Messages: server → client `{"mod":"ui","op":"open"|"close"|"entries", "owner": ...}`;
client → server `{"mod":"<owner>","op":"ui_event","event":{"window","node","event"}}`.
Mods live on their own **MODS tab** in the player menu (next to SKILLS; key **K**, also
while playing, `shroudlink_keys.json` `{"mods": "K"}` changes it): mod entries with their
ON/OFF switch on the left, the selected mod's windows on the right. A window may be longer
than the page: the page scrolls (mouse wheel, scroll bar), and only the parts in view are
drawn. Notifications (`notify`) show as framed messages for 8 s, at most 3 at a time (a
repeat counts up, "(×2)"): at the top while playing, at the bottom over menus, where a
click dismisses one.

## Built-in client features
- **Slot building blocks** (above): keys, marks, storage (`shroudlink_store.json` next to the
  game, per server and mod) and the backpack layout.
- **Item search on the chest screen:** "SEARCH ( / )" in the backpack panel header. Click it
  or press `/`, type: slots of both panels whose (localised) item name does not match turn
  grey while typing. `Esc` clears, a second `Esc` leaves the field, `X` clears.

## Status (Oct 2026, game build 26083076)
- Channel: verified end to end in game (beacon, claim, messages both ways, chunking).
- Native UI, verified in game: MODS tab in the player menu, page with mod list, text,
  headings, lists (multi-column), buttons and paging, item grids with real item icons and
  clicks, text fields (typing, Enter, Esc) with the keyboard kept from the game.
- Not clicked in game yet: toggles.
- Core: unit tests cover sessions, chunking, reconnects and loading `hello_mod.dll` through
  the C ABI (`cargo test -p shroudlink-server`); the chestsort mod tests its UI and locks
  with the SDK's mock host (`cargo test -p chestsort`).

## Lessons from the game's UI (keen UI)
- Frames are identified by a 64-bit key; siblings need unique keys, and a key must stay with
  the same widget across frames (derive it from the content, not a counter).
- A frame without a size fills its parent; give panels explicit sizes.
- Layers are identified by kind: one layer per kind and frame, drawn in the order of their
  kind: the inventory screen 6, the MODS page 0xd, the game's item tooltips 0xe (they open
  their own layer), popups 0x1d.
- Mod buttons test the cursor against their layout rectangle (`node+0x114`, UI units of a
  1920x1080 reference) instead of the game's interaction state, which misfires when a window
  is replaced from outside.
- Alignment (`node+0x6c`) places a frame in its parent, not its children: centre a label
  with padding.
- Panel backgrounds (`FUN_7ff6232fd910`, descriptor: +0 style, +4 opacity, +8 highlight,
  +0x68 two coloured rectangles, +0x98 their count): styles 4–6 reach past the frame on one
  side, 0 is the gold divider, and every style ends with a texture that masks what it drew
  (style 7 fades towards the top; a style without a texture masks everything away). Solid
  boxes use the game's rectangle fill (`FUN_7ff62304a800`, the 10th call in the background
  function) directly. Colours are 0xAABBGGRR and show brighter than written.

## Files
| path | role |
|---|---|
| `link/` | protocol + UI schema (shared) |
| `server/` | core (`shroudlink_server.dll`, installed as `steam_api64.dll`): hooks, inventory events, game API, mod loader with hot reload |
| `mods/chestsort/` | chestsort as a mod (`shroudlink_mods/chestsort.dll`) |
| `docs/sdk-v2.md` | design of the mod API |
| `client/` | client addon (`dinput8.dll`) |
| `sdk/` | mod SDK (C ABI + Rust trait) |
| `examples/hello_mod/` | example mod DLL |
