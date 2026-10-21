# Lantern: torch light with the weapon out (research)

## How the torch lights

`Tool_Torch01` / `Tool_Torch02_Resin` are `keen::ItemInfo` resources. Their light is not a
property of the item or the player: `ItemInfo.equipment` (`keen::EquipmentSetup`) names a
`visualEntity` (an entity template) that the client attaches to the character while the
item is equipped. That template carries the flame VFX and the render light (lights only
exist on the render side: `RenderLight`, `SceneLightResource`). The torch occupies a
hand slot (`EquipmentSlotType::Tool`), so the weapon cannot be out at the same time.

## Plan

An EML data mod (like the stack and chest mods), creating a new item:

1. Clone a ring's `ItemInfo` (slot `Armour_Ring`, two ring slots, the hands stay free)
   with `create_resource` / `add_resource` (both exist in EML's Lua API).
2. Give it a new id and Guid, a name ("Lantern") and the torch's
   `equipment.visualEntity`, keeping the ring's slot.
3. Add a recipe (or have a server mod hand it out with `World::move_stack` from a chest).

Open questions to answer in game:
- Where the attached template renders for a ring (the torch template may be positioned
  for the hand); the light itself is what matters.
- Whether the template's own impact/program (burning duration, fuel) runs outside a hand
  slot.

## The catch: clients need it too

An item that does not exist in the vanilla game must be known to every player's client:
the client builds the visuals and the UI from its own data. So unlike everything else so
far, the lantern is not server-only. Every player would need the patched client data
(EML's `--patch` on the client, redone after each game update), or the shroudlink addon
would have to learn to apply data patches itself. That is a decision for the server owner
before any client files are touched.
