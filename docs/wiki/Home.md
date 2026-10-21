# shroudlink

shroudlink is a mod framework for **Enshrouded 1.0 dedicated servers**. Server mods run on
the server and can talk to an optional client addon over the game's own netcode, which
draws their UI natively in the game. Players without the addon play normally.

It ships with **chestsort**, the mod it was built for:

- **Chests sort by name.** Rename a chest to e.g. "Food, Wood". The deposit button or a
  Quick Stack Station then puts every item of those categories into it.
- **Equipped items are never deposited.**
- **Lock slots** with `L` (addon): locked items never leave your backpack.
- **Item search in chests** with `/` (addon).
- Optional data mods: chests with 2× the slots, stacks 10× as large.

![The MODS tab added to the player menu](https://raw.githubusercontent.com/bjck/shroudlink/main/docs/screenshots/mods-tab.jpg)

## Where to start

| You are… | Read |
|---|---|
| a **player** on a server that runs shroudlink | [[Installing the addon\|Players-Installing-the-Addon]], [[Features\|Players-Features]] |
| a **server owner** | [[Server installation\|Server-Installation]], [[Chest categories\|Server-Chest-Categories]], [[Game updates\|Server-Game-Updates]], [[Troubleshooting\|Server-Troubleshooting]] |
| a **mod developer** | [[Getting started\|Modding-Getting-Started]], [[SDK reference\|Modding-SDK-Reference]], [[UI and building blocks\|Modding-UI-and-Building-Blocks]], [[Testing\|Modding-Testing]] |
| a **contributor** | [[Architecture]], [[Reverse engineering\|Reverse-Engineering]] |

## Downloads

See the [releases](https://github.com/bjck/shroudlink/releases):

- `shroudlink-client.zip` is for players. It contains the optional addon `dinput8.dll`.
- `chestsort-enshrouded.zip` is for server owners. It contains the core, the chestsort
  mod, the categories and the update script.

## Status

- Tested with server build 26083076 and client build 26082781.
- The server side is covered by a self-test that runs inside a real dedicated server
  (see [[Testing\|Modding-Testing]]).
- Not affiliated with Keen Games. Use at your own risk, and keep backups of your savegame.
