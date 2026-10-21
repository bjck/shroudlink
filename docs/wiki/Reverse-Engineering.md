# Reverse engineering

How the hook points were found, and how to find new ones. Tools are in
`research/scripts/`.

## Inputs

- **Server:** `enshrouded_server.exe` is a plain PE, so load it into Ghidra (headless
  project in `research/ghidra`).
- **Client:** `enshrouded.exe` is DRM-packed on disk. Dump the running process with
  `dumpmod.py <pid> enshrouded.exe out.exe`; sections land at their virtual addresses,
  with the live ImageBase.
- **Type data:** `kfc-parser extract-types` writes `reflection_data.json`, with every
  reflected struct, its fields and offsets (e.g. `keen::ecs::SlotSelection`: equipment at
  `+0x0`, actionbar pages at `+0xa0`). Query it with `reflect.py`.

## Tools

| Script | Use |
|---|---|
| `ghidra.sh DecompileAt.java <outDir> <addr…>` | decompile functions (`GP=client` for the client dump) |
| `ghidra.sh RefsTo.java …` / `StringXrefs.java` | references |
| `ripxref.py <exe> <target>` | fast RIP-relative xrefs without Ghidra, mapped to functions via `.pdata` |
| `disasm.py <exe> <va> [len]` | disassemble one function |
| `mksig.py <exe> <va…>` | shortest unique wildcarded signature for a function entry |
| `checksigs.py <server exe> [--client dump]` | check every shroudlink signature against a build |
| `memscan.py <pid> <text>` | find strings in a live process |
| `inject.py <process> <dll>` | hot-load the addon into the running game |
| `shot.py <out.png>` | screenshot of the game window |
| `categorize.py` | generate the default categories from the game's item data |

## Rules that kept things robust

- Locate functions by a **signature at the entry**. Take small helpers as the n-th
  `call` inside a known function: that survives patches better than a signature on a
  tiny function.
- Every server signature goes into `hooks.rs` with its feature. A comment above it names
  the original function and its parameters.
- Document struct offsets where they are used, with the reflected type and field name.
- Prefer the game's own accessors to raw memory layouts.
- After every change: unit tests, then `selftest.py`. For client changes, check in game.

## Techniques that worked

- **Event type hashes as constants:** `qualifiedHash` values from the reflection data
  show up as `mov edx, imm32` right before the event allocator. That is how the generic
  event allocator was found.
- **A hardware write watchpoint** (DR0 on all threads, plus a vectored exception handler
  that logs RIP when the value becomes 0) found the single function that empties the
  event queues, which is the dispatcher.
- **The world system that builds an inventory context without players**
  (`FUN_14019c480`) lets the self-test call inventory functions with no client at all.

## Things that were easy to get wrong

- Client and server **share** entity ids within a session; ids change between sessions.
- Quick stack (`FUN_1401ad1c0(out, ctx, to, from, …)`) runs in **both directions**.
- The R button (move all) is a different function from hold R (quick stack).
- A text field must swallow only key **downs**. Swallowing key releases leaves keys
  stuck in the game.
- Without players, almost no game systems run. The only no-player inventory context is
  built once, after the world has loaded (`FUN_14019c480`), which is what the self-test
  uses.
- Chests far from players are not loaded on the server, so you cannot find them by
  entity id at boot.
