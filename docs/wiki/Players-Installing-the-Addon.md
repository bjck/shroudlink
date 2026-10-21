# Installing the addon (players)

The addon is **optional**. Without it you can still join a shroudlink server, and chest
sorting by name works for everyone. The addon adds the MODS tab, the item search, slot
locks and whatever UI the server's mods offer.

## Install (1 minute)

1. Download `shroudlink-client.zip` from the
   [releases](https://github.com/bjck/shroudlink/releases).
2. In Steam, right-click Enshrouded, then **Manage → Browse local files**.
3. Copy `dinput8.dll` into that folder, next to `enshrouded.exe`.
4. Start the game.

## Uninstall

Delete `dinput8.dll` from the game folder.

## Good to know

- **Other servers:** the addon stays completely silent on servers without shroudlink.
- **Game updates:** if an update changed the game too much, the addon switches itself
  off and the game keeps working normally. A new addon version is needed then.
- **Files it creates** in the game folder:
  - `shroudlink_client.log` records what the addon did. Look here when something seems
    off.
  - `shroudlink_store.json` holds the data mods keep on your PC, per server (e.g. your
    slot locks).
- **Updating:** close the game and replace `dinput8.dll`. While the game runs, Windows
  locks the file.

## Versions

A new addon also works on a server with an older core: you get no slot locks there, but
everything else works. An older addon works on a newer server: its locks are still
honoured.
