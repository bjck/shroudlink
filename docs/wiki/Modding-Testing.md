# Testing

## Unit tests (no game)

`shroudlink_sdk::testing::MockHost` records what your mod sends:

```rust
let mut m = MyMod::default();
let mut host = MockHost::default();
m.on_join(&mut host.host(), 1);
let sent = host.take_sent();            // Vec<(client, Value)>
assert_eq!(sent[0].1["op"], "entries");
```

```
cd mod
cargo test --workspace --exclude shroudlink-client
```

## Self-test inside a real dedicated server (no game client)

```
cd mod && cargo build --release && cd ..
python research/scripts/selftest.py
```

The runner:
1. backs up the local server's `savegame`;
2. installs the freshly built core and mods;
3. starts the server with a marker file;
4. waits for `shroudlink_selftest.txt`, stops the server and restores the savegame.

It takes about a minute. The exit code is 0 when everything passed.

```
PASS hook points: 9 found, features: channel, inventory, move_all, selftest
PASS addon session: welcome lists [{"id":"chestsort",...},{"id":"hello",...}]
PASS slot key offered · PASS MODS menu entry · PASS backpack section offered · PASS notification
PASS L stores the lock on the PC · PASS L marks the slot
PASS quick stack sorts by category: 7 iron bars went into the chest named Ores
PASS quick stack keeps locked slot · PASS move all keeps locked slot
PASS hot reload: chestsort reloaded and greeted the session again
RESULT ok
```

**How it works without a player.** Inventory functions need a game context. After the
world has loaded, one world system builds such a context from world-level data. The core
copies that system's descriptor, builds its own context and runs the tests on the game
thread:

- a fake addon session on a free player slot;
- test stacks in an inventory that plays the player;
- a test name ("Ores") for another inventory;
- the game's own quick stack and move all, through the real hooks and mods.

**Simulate a game update that broke a hook point:**

```
python research/scripts/selftest.py --break move_all
```

Only that feature may switch off. The test reports which features stayed on.

**Not covered:** anything that needs a real player (equipment protection), and how the
addon draws things. Check those in game.

## Automated in-game tests

With a file `shroudlink_automation` in the game folder, the addon reports what is on
screen on `127.0.0.1:47911`, only locally:
- screen id and connection;
- mod menu, windows and backpack sections;
- HUD messages and the search;
- every backpack slot with its item, mark and screen rectangle.

`research/scripts/gamectl.py` drives the game with real keyboard and mouse input and
checks the result through that interface:

```
python research/scripts/gamectl.py test all     # fully automatic, see below
python research/scripts/gamectl.py state        # what the addon sees
python research/scripts/gamectl.py ensure       # in the world on the local server (rejoins if needed)
python research/scripts/gamectl.py goto food    # teleport to the chest named "food" and open it
```

`test all` needs no one at the keyboard; the local server needs a `shroudlink_probe` file
for the core's test commands:

1. It makes sure the game is in the world. If not, it starts it through Steam with
   `+connect` / `+password` and confirms "Join session".
2. It teleports the character to the chest. It finds the camera direction by walking a
   step with W and measuring the move on the server, then places the character with the
   chest straight ahead of the camera.
3. It runs the chest scenario.

`test chest` opens the chest (E), tests the search (`/`, typing, Esc, Esc), locks a slot
(L, rim), quick stacks (hold R), checks that the locked item stayed, unlocks it and closes
the chest. Characters are typed as Unicode input, so any keyboard layout works.

## In game

1. Run a local server (`server/enshrouded_server.exe`, port 15637) and join
   `127.0.0.1:15637`.
2. Load a new addon build into the running game:
   ```
   python research/scripts/inject.py enshrouded.exe mod/target/release/shroudlink_client.dll
   ```
   The new copy shuts the old one down.
3. Take screenshots with `python research/scripts/shot.py out.png`.
