# EML research scripts

Read-only EML mods (`"capabilities": ["export"]`) that dump game data. Copy one into the
local server's `mods/` folder and run

    tools/eml/emm.exe run -g server --export --export-directory <out>

Remove it from `mods/` afterwards (the patch run of `UPDATE-SERVER.ps1` would run it too).
The dedicated server ships without localisation, so names are debug names.

- `lantern_dump`: torch and ring items, their visual templates, buffs (see
  `docs/research/lantern.md`).
- `travel_dump`: fast-travel templates (by `MapMarker` type) and where the world places
  them (`SceneEntityChunkResource`; see `docs/research/travel.md`).
