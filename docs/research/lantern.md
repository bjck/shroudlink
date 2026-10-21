# Lantern: a light with the weapon out (research)

## How the torch lights

`Tool_Torch01` / `Tool_Torch02_Resin` / `Tool_Torch03_Wax` are `keen::ItemInfo` resources
in slot `MeleeWeapon`. Their light is not item data: `ItemInfo.equipment.visualEntity`
names a template (`Visuals_Club_Fire_Wooden01_Torch`, …) that is attached to the hand
while the item is equipped, with `VfxComponent` + `SlotAttachment`. The flame VFX carries
the light. The torch takes the weapon hand, so a weapon cannot be out at the same time.

## The game already has a body light: the Ring of Light buff

`Ring_T3_Ring_of_Light` (and `Ring_T6_Ring_of_Shock`, `Ring_T7_Ring_of_Fire`, which share
the visual) has `equipment.visualEntity = Visuals_Ring_Of_Light`
(`1890618a-992e-46b1-ad68-51bd8bf5e3a0`). That template has no model, only an `Impact`
whose program (`5867aa21-…`, variables `GetRoot`, `GetEntityParent`, `ResetRate`,
`BuffType`) applies a buff to the wearer every 0.1 s:

| | |
|---|---|
| BuffType Guid | `b2f8db72-a69d-43db-baf1-c69937ec0264` (`BuffTypeId` 2348168016) |
| slot / apply type | Buff / ResetTime, lifetime 0.3 s (`keen::Time` is in nanoseconds), hidden in the UI, not saved |
| `whileApplied` | VFX `826ea0d6-2181-4fea-8aba-3491cd40c39d` on the Chest slot, +0.57 m up, with light parameters |

So wearing one of these rings already gives a light on the chest with both hands free.
Found with an EML export script (`research/scripts/eml/lantern_dump`).

## Server-only lantern: applying that buff from a mod

Buffs are server state (`ecs::BuffContainer`, buff entities with `BuffNetworkData`), and
clients draw a buff's `whileApplied` effects from their own data. A vanilla buff applied
by the server therefore needs no client files. The core applies buffs through the game's
impact VM (`server/src/buffs.rs`):

- natives are registered by name in `FUN_14022e680` (`applyBuff` -> `FUN_1401775c0`,
  `getSelf` -> `FUN_140182290`, also `removeBuff`, `hasBuff`, `spawnEntity`, …; the
  log string `[apply buff] Unknown Buff '%k'` sits in `applyBuff`);
- a native is `fn(out, args, vm)`: `args` -> u32 variable indices, `vm+0` the variable
  layout (`ImpactVariable`, offset at +0xe), `vm+0x10` the data, `vm+0x30` the running
  program's context (its entity, component storages), valid only while it runs;
- the core queues requests and runs `applyBuff(guid, target)` from inside `getSelf`, with
  the running context and its own variables. A kept buff is re-applied every 0.1 s, as
  the ring does (`World::apply_buff(entity, guid, seconds)`). The third native argument
  is not a lifetime: with it set, no buff entity appears.

The `lantern` mod (MODS -> Lantern) keeps the buff for 30 s and renews that every 10 s
from `on_tick`; Light off ends it at once.

**Verified in game (2026-10-21, local server):** with the light on, a buff entity with
`BuffComponent.buffTypeId` 2348168016 exists for as long as the light is on and is gone
1.5 s after Light off. At night the grass around the character is lit, with the unmodified
client (`docs/screenshots/lantern.jpg`, top: off, bottom: on). Without players no impact
program runs on a dedicated server, so the self-test only checks that the hooks are found.
Probe ops for this: `debug_buff` (`buff`, `seconds`), `debug_scan` (entities with a
component type), `debug_component` (a player's component).

## Rejected: a new lantern item

Cloning a ring's `ItemInfo` with the torch visual would need the new item in every
player's client data (EML `--patch` on each client after every game update).
