-- Research: the harvest of crops. A ripe stage's `DefaultInventory` is applied only when
-- a player picks it (scatter loot like beans, and wheat, corn …), so the stored inventory
-- is empty. Dumps those `DefaultInventoryResource`s. See docs/research/assistants.md.
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

-- the ripe-stage inventories of every crop in plants.tsv
local wanted = { ["711ab8e1-2101-40d5-bb9f-727b6711725a"] = true, ["304ab9b0-dbe8-451e-8eb0-8f238c286cf5"] = true, ["87512047-0e21-4616-bbde-1a6e617d65d5"] = true, ["925375d4-04ba-450d-821a-44581c3dd7bb"] = true, ["cb6182e4-ee58-42b3-9bb5-d19b714a3988"] = true, ["9668b921-adf5-406b-9004-463288540dbc"] = true, ["a745e1d8-9e93-43aa-b49f-ba2f3d4a09d0"] = true, ["1c749952-a99c-4f62-9ba7-febdf4571f72"] = true, ["d66ed066-4f09-4944-b614-05b67fb2d3a5"] = true, ["1bd720c3-be1e-4d4f-8d3c-e3aae5fe0744"] = true, ["caa04b20-093a-4951-be2f-7c9a2675d4f3"] = true, ["bdae0d9d-2e42-4d4d-b07c-61a240f822a2"] = true, ["524c18e3-af3a-4684-a358-6961d67d95b6"] = true, ["08dedbb6-53a8-40e2-a95a-aee1b811c509"] = true, ["de1d619b-7862-4adf-8529-ffc6fb85ef49"] = true, ["e224c46f-2370-49c0-80d0-3a0a34f6bb95"] = true, ["240f0e2b-f240-46f5-a0ba-f527e2e38e5a"] = true, ["44e3569b-a894-4fa7-98a2-8d8b36bed2ff"] = true, ["faa21dd8-0313-4fe7-841e-471ab69594ee"] = true, ["e3f79d46-b927-429c-8528-79d0e17f5ffc"] = true, ["6bc9d1b2-182c-408e-9aea-fdcbaddeef64"] = true, ["e3116db3-f257-4204-a77d-987353aeb916"] = true, ["cd7520de-4c11-43f6-be0c-73ef52da583c"] = true, ["3d1262d2-3616-4e8b-951a-62c0d495a002"] = true, ["dfa9523d-dd9e-41ec-b834-b437c6c3bc5b"] = true, ["5e892fee-107b-4813-9a7a-ff2592918449"] = true, ["b69b9bd2-5d11-4e7b-b0ba-616db3f9b37e"] = true, ["25c3d55c-c3e6-4a34-a0b5-9ea63535483e"] = true, ["f679bc97-07cd-4668-a944-94bc199d19e1"] = true, ["ef461dd8-4de9-4a25-97b7-8516645ccff1"] = true, ["7174e65c-060a-4cdf-bf6f-df00cebe1c8d"] = true, ["240d4540-9393-46df-8728-8425be75e389"] = true, ["bd20bb16-9b0f-49c6-982e-052ccb9071c7"] = true, ["18280ed9-7665-445f-ae97-8dadd3d820bb"] = true, ["c882e43a-fa24-4dc2-95c9-ea7779acf4db"] = true, ["1cfeb4ea-321d-4399-bd00-55d740ac00cc"] = true }
for _, res in ipairs(game.assets.get_resources_by_type("keen::ecs::DefaultInventoryResource")) do
	if wanted[tostring(res.guid)] then
		p(("INVENTORY %s %s"):format(tostring(res.guid), dump(res.data)))
	end
end
io.export("croploot.txt", table.concat(out, "\n"))
