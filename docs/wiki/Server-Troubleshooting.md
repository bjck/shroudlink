# Troubleshooting

## Files to look at (server folder)

| File | Shows |
|---|---|
| `shroudlink.log` | everything the core and mods did: startup, mods loaded, addon sessions, sorting (`chest 6972 'food' ["Food"]: moved 61 item(s) by category`), locks kept |
| `shroudlink_hooks.txt` | which features are on, and every hook point with its address or the reason it is missing |
| `logs/enshrouded_server.log` | the game's own log (look for `Deleted … items`) |

On the player's PC, `shroudlink_client.log` in the game folder shows what the addon did.

## Common problems

**Nothing is sorted.**
- Is the chest really named? Look for `no category in name, vanilla only` in the log.
- Does the name contain a category word? See [[Chest categories|Server-Chest-Categories]].
- Do you see `features: … inventory OFF` in the log? Then a game update broke the
  inventory hooks, and a new core version is needed.

**`shroudlink.log` does not exist.**
The core is not loaded. Check that `steam_api64.dll` is the core (about 1 MB) and that
Steam's original is present as `steam_api64_o.dll`.

**The MODS tab is missing for a player.**
- Is the addon installed (`dinput8.dll` next to `enshrouded.exe`)?
- Does the player's `shroudlink_client.log` show `connected, server mods: …`?
- Does `shroudlink_hooks.txt` show `ON channel`?

**Locked items are still deposited.**
- Has the player opened a chest once since joining? That is when the addon learns the
  backpack layout.
- `shroudlink.log` should show `player N: K locked slot(s) kept` on every deposit.

**The server crashes on start after an update.**
The data files are probably for another build. Follow
[[Game updates|Server-Game-Updates]].

## Switching shroudlink off

Rename `steam_api64.dll` to `steam_api64.shroudlink.dll` and `steam_api64_o.dll` back to
`steam_api64.dll`. Your world keeps everything, but with vanilla data files, stacks
above the vanilla limit are cut on load (see [[Game updates|Server-Game-Updates]]).
