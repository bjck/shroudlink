# Updraft charges and extra jumps (2026-11-01)

Goal: mod-granted movement abilities (Paragon "Updraft boost": +2 updrafts per glide; a
triple jump), as a generic addon building block, not Paragon code.

## Findings [V: reflection data, research/server/reflection_data.json]

- There is no updraft counter: one updraft per glide, then
  `keen::ecs::LocomotionFlags::IsUpdraftDepleted` (bit 6) is set.
- `keen::ecs::DynamicLocomotion` (456 bytes): +0x59 `state` (`LocomotionState`: 0 Walking,
  2 Falling, 4 HangGliding, 7 ClimbingWall, 9 Swimming …), +0x5a `previousState`, +0xb8
  `lastJumpTime`, +0xf8 `flags` (`LocomotionMask`, u16; bit 6 IsUpdraftDepleted, bit 1
  Sprinting), +0x154 `jumpType` (`actor::JumpType`, Glider is one), +0x155 `jumpCount` u8,
  +0x188 / +0x18c glider upstream factors.
- `GliderConfig` (in `EquipmentSetup` +0x3bc): +0x1c `updraftStrengthChargeStartValue`;
  the player setting `quickUpdraftCharge` charges the updraft by holding.
- The server's component table has no `DynamicLocomotion` (only `Locomotion`,
  `NetworkLocomotion`, `ClientLocomotion`, `PlayerLocomotion`, `SteeringLocomotion`): the
  player's movement is simulated by the client, so is the updraft flag and the jump count.

## Plan

A client building block, e.g. `ToClient::Movement { owner, extra_updrafts, extra_jumps }`
for the local player: while `state` is HangGliding and bit 6 of `flags` gets set, the
addon clears it again up to `extra_updrafts` times per glide (reset when the glide ends);
while airborne, when `jumpCount` reaches the game's limit, it lowers it once per extra jump
(reset on landing). Needed first: the client's component lookup for the local player's
`DynamicLocomotion` (the client ECS is the same engine as the server's: descriptor table
by type hash, entity record → archetype → component array, as `server/src/components.rs`),
and whether the double jump's limit is `jumpCount` against a value from the skill.
