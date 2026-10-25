-- Research: the harvest of crops whose ripe stage is scatter loot (beans, pumpkins …):
-- their `DefaultInventory` is applied only when a player picks them, so the stored
-- inventory is empty. Dumps those `DefaultInventoryResource`s. See docs/research/assistants.md.
if not loader.features.export then return end
local out = {}
local function p(s) out[#out + 1] = s end
local function dump(v, depth)
	depth = depth or 0
	if depth > 12 then return "..." end
	if type(v) ~= "table" and type(v) ~= "userdata" then return tostring(v) end
	local parts = {}
	local ok = pcall(function()
		for k, x in pairs(v) do parts[#parts + 1] = tostring(k) .. "=" .. dump(x, depth + 1) end
	end)
	if not ok or #parts == 0 then
		local okv, val = pcall(function() return v.value end)
		if okv and val ~= nil then return dump(val, depth + 1) end
		return tostring(v)
	end
	table.sort(parts)
	return "{" .. table.concat(parts, " ") .. "}"
end

-- the inventories of the scatter-loot crops in plants.tsv
local wanted = { ["cb6182e4-ee58-42b3-9bb5-d19b714a3988"] = true, ["cd7520de-4c11-43f6-be0c-73ef52da583c"] = true, ["b69b9bd2-5d11-4e7b-b0ba-616db3f9b37e"] = true, ["25c3d55c-c3e6-4a34-a0b5-9ea63535483e"] = true, ["f679bc97-07cd-4668-a944-94bc199d19e1"] = true, ["ef461dd8-4de9-4a25-97b7-8516645ccff1"] = true, ["240d4540-9393-46df-8728-8425be75e389"] = true, ["bd20bb16-9b0f-49c6-982e-052ccb9071c7"] = true, ["18280ed9-7665-445f-ae97-8dadd3d820bb"] = true, ["c882e43a-fa24-4dc2-95c9-ea7779acf4db"] = true, ["1cfeb4ea-321d-4399-bd00-55d740ac00cc"] = true }
for _, res in ipairs(game.assets.get_resources_by_type("keen::ecs::DefaultInventoryResource")) do
	if wanted[tostring(res.guid)] then
		p(("INVENTORY %s %s"):format(tostring(res.guid), dump(res.data)))
	end
end
io.export("croploot.txt", table.concat(out, "\n"))
