# The game's skill tree page, for mod skill trees (client build of 2026-11-01)

Goal: a generic SDK building block `skill_tree` (a mod sends nodes, links, ranks; the addon
shows them with the game's own SKILLS page; clicks come back as `ui_event`). Nothing here is
specific to Paragon. Legend: [V] read in the decompile, [I] inferred, [?] open.

Dump: `research/clientdump/enshrouded_dump_20261101.exe` (base 0x7ff7edb20000, not in the
repository), Ghidra project `research/ghidra_client_1101`, decompiles in
`research/decomp_client_1101/`. Addresses below are of that dump; RVA = address - base.

## Data the page reads [V]

- `game_ui` = `*(base + 0x281e498)` (the addon's "game ui"). `*(game_ui + 0xd0)` -> player
  skill UI object; `*it` -> `p`; `*(p + 0x38)` -> player data `d`; the runtime skill tree is
  at `d + 0x449198` (int offset 0x112466 * 4).
- Runtime tree (`g38_skilltree::SkillTree` [I]): +8 a hash set of nodes (iterated with
  `FUN_7ff7edb5f070` begin / `FUN_7ff7edb5f270` next, end at +0x10), +0x40 node array of
  0x44 bytes (+0 node id, +4 byte: 0 = in the root list), +0x58 links (16 bytes:
  +4 source id, +8 target id = `keen::SkillTreeNodeLink`), +0x60 link count.
- Reflection (`research/server/reflection_data.json`): `SkillTreeResource { nodes:
  blob_array<SkillTreeNode (140 bytes)>, links: blob_array<SkillTreeNodeLink (16)> }`;
  `SkillTreeNode`: +0 id, +4 uiPosition float2, +0xc name, +0x10 description, +0x14
  perLevelEffectDescription (loca keys), +0x2c iconInactive / +0x3c iconActive (UiTexture,
  16 bytes), +0x84 costs u16, +0x86 type (Root/Attribute/ActiveSkill/PassiveSkill), +0x88
  maxLevel, +0x89 levelIndex, +0x74/+0x7c incoming/outgoing links.

## The page [V]

- `FUN_7ff7ee7c7970(mode, menu?)` (RVA 0xca7970): the SKILLS page, called every frame by
  the player menu. Gets its state (0x5e00 bytes) from the UI frame storage
  (`FUN_7ff7eeca5580(.., 0x6740b65c5cae6634, 0x5e00, ..)`), logs "[GameUi] Skill Tree
  opened" on the first frame, then `FUN_7ff7ee8610e0(state, tree)`.
- `FUN_7ff7ee8610e0(state, tree)` (open): builds the page's own lists from the runtime tree:
  node entries at state +0x78, 0x50 bytes each, count u16 at +0x5078 (max 0x100); root
  entries (pointers) at +0x5080, count byte at +0x5180 (max 0x20); link index pairs at
  +0x5188 (u16 a, u16 b), count u16 at +0x5988 (max 0x200); each node entry keeps up to 8
  neighbour indices at entry +0x30 (count byte at +0x40). Entry: +0 node id, +4/+8 screen
  position (filled each frame by `FUN_7ff7ee8ad280`), +0x18 0x7f7fffff, +0x1c flag,
  +0x48 pointer to the runtime node (0x44 bytes). Stores the tree at state +0x5df8; the page
  draws only while that is set.
- Per frame: `FUN_7ff7ee7c2550` ("nodesParent") loops the entries and calls
  `FUN_7ff7ee7c20a0(state, entry, index)` per node (hover / selection: state +0x58 hovered,
  +0x68 selected, +0x50 …), which sizes it (`FUN_7ff7ee841300`, `FUN_7ff7ee84b4b0`) and draws
  it with `FUN_7ff7ee816ea0(entry, state, 1)` (+ `FUN_7ff7ee81a570` when runtime node +0x40
  is set). Links: "drawNodeLinks", `FUN_7ff7ee8176c0(.., state, pair)`. Tooltip:
  `FUN_7ff7ee7c2950` ("SkillNodeTooltipWindow", lockIcon, unlockedIcon, level). Top bar with
  the points: `FUN_7ff7ee7c69b0` ("chromeLayer"); cursor: `FUN_7ff7ee7c7270`. Class labels:
  `FUN_7ff7ee685320` ("ClassHints").

## Runtime node, read live [V]

The game-UI global is only set while the UI builds a frame; read in a loop it gives the
chain above. The tree held 312 links; nodes are 0x44 bytes:

| Offset | What | Example |
|---|---|---|
| +0 | node id | 0x53e61fff |
| +4 | type (`SkillNodeType`: 0 root, 1 attribute, 2 active, 3 passive) | 3 |
| +8, +0xc | two loca keys (name, description) [I] | |
| +0x10 | per-level effect key [I], often 0 | |
| +0x14, +0x18 | UI position (f32 x, y) | 5030, 4593 |
| +0x1c | two u16 (cost, max level) [I] | 1, 1 |
| +0x20 .. +0x3f | impact data, active skills only [I] | |
| +0x40 | flags; non-zero draws an extra layer (`FUN_7ff7ee81a570`) | 0x01000103 |

`FUN_7ff7ee816ea0(entry, state, 1)` draws a node from the runtime node alone (entry +0x48)
[V]: +4 type (2 = active skill: a large icon; 1 = attribute and others: the small dot),
+0x1e class (0 / 1 / 2 / other: four frame colour sets at game_ui+200 +0xe8b8 … +0xe8f0),
+0x20 icon inactive and +0x30 icon active (16-byte `UiTexture`, active skills), +0x43
unlocked (picks the active icon and full brightness); entry +0x28 another highlight flag.
So the icon, the frame and the unlocked state are all in the runtime node: the game fills
them from its data and the player's unlocks, and a mod node can carry its own. +0x40 /
+0x41 are probably level / max level [I] (node 0 read 3 / 1).

## Plan

1. [?] What the tooltip (`FUN_7ff7ee7c2950`) reads for name, description and rank, and what
   a click / "invest" sends (the `UnlockSkillAction` path) so the addon can catch it.
2. While a mod tree is shown: call the page ourselves (as the MODS tab does with the tab
   widget), and right after the open replace the node entries, roots and link pairs with
   ones built from the mod's nodes, each pointing at a runtime node the addon owns (icon and
   loca keys copied from a game node the mod names; texts through the existing loca hook).
3. Clicks: the unlock action the page sends for a node (to find: `UnlockSkillAction`) is
   caught for mod nodes and becomes `ui_event { window: tree id, node: node id, click }`.
4. Protocol: `ToClient::SkillTree { owner, id, title, points, nodes[{id, x, y, name,
   description, rank, max_rank, icon (game node id), available}], links[{from, to}] }`;
   a `Tab` with the same id shows the tree instead of windows.
