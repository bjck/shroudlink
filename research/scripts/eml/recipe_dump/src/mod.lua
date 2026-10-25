-- Research: recipes (inputs, outputs, duration, workshop), input categories, workshops and
-- NPCs (which assistant belongs to which workshop). See docs/research/assistants.md.
if not loader.features.export then return end
local out = {}
local function p(s) out[#out + 1] = s end

-- HashKey32, Time and typedefs may come as tables with a `value` field
local function n(v)
	for _ = 1, 4 do
		if type(v) ~= "table" and type(v) ~= "userdata" then break end
		local ok, inner = pcall(function() return v.value end)
		if not ok or inner == nil then break end
		v = inner
	end
	return tostring(v)
end
local function list(v)
	local ok, values = pcall(function() return v.values end)
	if ok and values ~= nil then return values end
	return v
end

local function first(ty)
	local ok, res = pcall(game.assets.get_resources_by_type, ty)
	p(("# %s: %s"):format(ty, ok and tostring(#res) or ("error " .. tostring(res))))
	return ok and res or {}
end

for _, res in ipairs(first("keen::RecipeRegistryResource")) do
	for _, c in ipairs(list(res.data.inputCategories)) do
		local items = {}
		for _, it in ipairs(list(c.items)) do items[#items + 1] = n(it.itemId) .. ":" .. n(it.scaling) end
		p(("CATEGORY\t%s\t%s"):format(n(c.inputCategoryId), table.concat(items, ",")))
	end
	for _, r in ipairs(list(res.data.recipes)) do
		local inputs, outputs = {}, {}
		for _, i in ipairs(list(r.input)) do
			local count = tonumber(n(i.itemStack.count)) or 0
			if count > 0 then
				inputs[#inputs + 1] = "item:" .. n(i.itemStack.item) .. ":" .. count
			else
				inputs[#inputs + 1] = "cat:" .. n(i.inputItemCategory.category) .. ":" .. n(i.inputItemCategory.count)
			end
		end
		for _, o in ipairs(list(r.output)) do outputs[#outputs + 1] = n(o.item) .. ":" .. n(o.count) end
		p(("RECIPE\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s	%s"):format(
			n(r.recipeId), n(r.workshopId), n(r.craftingDuration), tostring(r.isCookingRecipe), tostring(r.isUpgrade),
			n(r.requiredEnergy), n(r.waterAmount), n(r.requiredPropsMask), n(r.level), n(r.comfortLevel),
			n(r.serverProgressLevel), n(r.requiredHappyNpcCount), n(r.debugName),
			table.concat(inputs, ","), table.concat(outputs, ","), n(r.waterCharges)))
	end
end

for _, res in ipairs(first("keen::WorkshopRegistryResource")) do
	for _, kind in ipairs({ "npcs", "workshops" }) do
		for _, w in ipairs(list(res.data[kind])) do
			p(("WORKSHOP\t%s\t%s\t%s\t%s\t%s"):format(kind, n(w.workshopId), tostring(w.isNpc), n(w.item), tostring(w.workshopGuid)))
		end
	end
	for _, c in ipairs(list(res.data.craftingProps)) do
		p(("PROP\t%s\t%s\t%s"):format(n(c.craftingPropId), n(c.craftingPropIndex), n(c.item)))
	end
end

for _, res in ipairs(first("keen::NpcCollection")) do
	for _, npc in ipairs(list(res.data.npcs)) do
		p(("NPC\t%s\t%s\t%s\t%s\t%s\t%s"):format(n(npc.id), tostring(npc.isCraftingAssistant), n(npc.workshopId), n(npc.itemId), n(npc.npcRoamingRange), n(npc.debugName)))
	end
end

-- game settings: the production speed factor the server runs with
for _, res in ipairs(first("keen::GameSettingsPresetsResource")) do
	p("SETTINGS " .. tostring(res.guid))
end

io.export("recipes.txt", table.concat(out, "\n"))
