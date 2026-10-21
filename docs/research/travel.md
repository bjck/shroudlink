# Travel to spires and fast-travel points (research)

## Where they are

Fast-travel destinations are templates with a `keen::ecs::MapMarker` component. In this
build, their marker types are:

| Marker type Guid | Templates |
|---|---|
| `77997dbc-4c4c-4843-a616-107505e4512d` | `Teleport_Platform_SavePoint_Gameplay` (70 placed), `…_Tower_Gameplay` (27, the spires), `…_FallenCrypt_Gameplay` (32), `Teleport_Platform_Vorgoth_Gameplay` |
| `2bad3206-cad7-469a-a358-64ea99181416` | `MapMarker_Tower_<region>_FastTravel` (one per region) |
| `5ebdad4b-08b6-4578-8d63-fe9917449eaf` | the Ancient Lands towers |

Player-built Flame Altars (`658ad7a1-…`) are savegame entities, not world data.

The world places static entities in `keen::SceneEntityChunkResource` (1601 chunks):
`templates[]` plus `entities[] { templateIndex, transform }`. `SceneEntityTransform`
stores each axis as two u16 values, 16.16 fixed point in absolute world metres:
`x = positionX1 + positionX0 / 65536` (y is up). For example, a spire platform is at
4194, 811, 4350. Export: `research/scripts/eml/travel_dump`.

The server only loads entities near players, so a mod cannot read a distant spire's
entity, but it can use these positions.

## Open: which points a player has discovered

Teleporting to any point would also skip exploration. The game tracks discovery through
its knowledge system (`KnowledgeGained` with `mapMarkerEntityId`,
`GameKnowledgeRequirement`), not yet researched. Until then, a Travel list of spires would
have to be either unrestricted (a server-owner decision) or limited to points the player
has stood at while the mod was running.
