# Getting started with mods

A shroudlink mod is a DLL in the server's `shroudlink_mods/` folder. It can:

- show windows in the players' MODS tab and react to clicks and text input;
- take part in every inventory transfer: keep slots, move stacks itself;
- react to game events: deaths, pickups, level-ups, crafting, building and 125 more;
- use the addon's building blocks: slot hotkeys, slot marks, data stored on the player's
  PC, rows in the chest screen, HUD messages.

The addon on the players' side is generic, so a new mod never needs an addon update.

## Requirements

- Windows, with Rust (stable, `x86_64-pc-windows-msvc`).
- A local dedicated server for testing (`steamcmd +app_update 2278520`), with the core
  installed (see [[Server installation|Server-Installation]]).

## Create a mod

```
git clone https://github.com/bjck/shroudlink
cd shroudlink
python research/scripts/newmod.py keep_torches
cd mod
cargo test -p keep_torches
cargo build --release -p keep_torches
```

`newmod.py` creates `mod/mods/keep_torches/` with a MODS tab window, inventory event
stubs and a unit test, and adds it to the workspace. Copy
`target/release/keep_torches.dll` into the server's `shroudlink_mods/`.

## A complete mod in 15 lines

```rust
use shroudlink_sdk::{export_mod, Game, InventoryEvent, Mod};

const TORCH: u32 = 123456; // item id

#[derive(Default)]
pub struct KeepTorches;

impl Mod for KeepTorches {
    fn id(&self) -> &'static str { "keep_torches" }

    // never quick stack or move torches out of a player's inventory
    fn keep(&mut self, game: &Game, event: &InventoryEvent) -> Vec<u64> {
        if !game.is_player(event.from) { return Vec::new(); }
        game.slots(event.from).iter().filter(|s| s.item == TORCH).map(|s| s.id).collect()
    }
}

export_mod!(KeepTorches);
```

## Developer loop

- **Hot reload:** overwrite the DLL in `shroudlink_mods/` while the server runs. It is
  reloaded within two seconds while a player is connected (`reloaded mod '…'` in
  `shroudlink.log`), and connected players get your `on_join` again.
- **Unit tests** with `shroudlink_sdk::testing::MockHost`, without the game.
- **Self-test** inside a real server, without a game client: see [[Testing|Modding-Testing]].
- **Logging:** `shroudlink_sdk::log!("…")` writes to `shroudlink.log`.

## Learn from real mods

- `mod/examples/hello_mod`: a window with a toggle and a counter, a "Wave" button in the
  chest screen and a HUD message.
- `mod/mods/chestsort`: categories, sorting via `on_transfer`, slot locks via `keep` and
  the building blocks, a UI with search and paging, and tests.

Next: [[SDK reference|Modding-SDK-Reference]] and
[[UI and building blocks|Modding-UI-and-Building-Blocks]].
