-- Research: crops (templates with Growth: growth, harvest, loot), what a seedling item
-- places, and fishing (spawn tables, fishable and bait items). See docs/research/assistants.md.
if not loader.features.export then return end
local out = {}
local function p(s) out[#out + 1] = s end

local function dump(v, depth)
	depth = depth or 0
	if depth > 9 then return "..." end
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

local skip = { ["keen::ecs::ModelComponent"] = true, ["keen::ecs::ModelResource"] = true, ["keen::ecs::VfxComponentResource"] = true,
	["keen::ecs::ColliderResourceComponent"] = true, ["keen::ecs::AnimationGraphResourceComponent2"] = true, ["keen::ecs::RenderTransform"] = true }
local shown = 0
for _, res in ipairs(game.assets.get_resources_by_type("keen::ecs::TemplateResource")) do
	local grows = false
	for _, c in ipairs(res.data.components) do
		if tostring(c.type) == "keen::ecs::Growth" then grows = true end
	end
	if grows then
		p(("PLANT %s %s"):format(res.data.name, tostring(res.guid)))
		if shown < 12 then
			for _, c in ipairs(res.data.components) do
				local t = tostring(c.type)
				if not skip[t] then p("  " .. t .. " " .. dump(c.value)) end
			end
			shown = shown + 1
		end
	end
end

local seen = 0
for _, res in ipairs(game.assets.get_resources_by_type("keen::ItemInfo")) do
	local d = res.data
	local name = tostring(d.debugName)
	if name:find("Seedling") or name:find("Seed_") then
		if seen < 2 then p("SEEDITEM " .. name .. " " .. dump(d, 2)) end
		seen = seen + 1
		p(("SEED %s %s"):format(tostring(d.itemId.value), name))
	end
	local okf, fish = pcall(function() return d.fishableItemSetup end)
	if okf and fish ~= nil then
		local s = dump(fish)
		if s:find("true") then p(("FISHITEM %s %s %s"):format(tostring(d.itemId.value), name, s)) end
	end
	local okb, bait = pcall(function() return d.baitItemSetup end)
	if okb and bait ~= nil then
		local s = dump(bait)
		if s:find("true") then p(("BAITITEM %s %s %s"):format(tostring(d.itemId.value), name, s)) end
	end
end

for _, ty in ipairs({ "keen::FishSpawnTableResource", "keen::fishing::FishSpawnTableResource", "keen::FishSpawnTableDirectory" }) do
	local ok, list = pcall(game.assets.get_resources_by_type, ty)
	p("# " .. ty .. " " .. tostring(ok and #list or list))
	if ok then
		for i, res in ipairs(list) do
			if i <= 40 then p("FISHTABLE " .. tostring(res.guid) .. " " .. dump(res.data, 0)) end
		end
	end
end
-- growth stages: Upgrade.levels[].upgradeEntity, and what the last stage is made of
local byguid = {}
for _, res in ipairs(game.assets.get_resources_by_type("keen::ecs::TemplateResource")) do byguid[tostring(res.guid)] = res end
local want = { T2_Plant_Growth_Beets = true, T1_Plant_Growth_Corn = true, T1_Plant_Growth_Tomato = true, T1_Plant_Growth_BerryBushes = true, T4_Plant_Growth_DatePalm = true, T3_Plant_Growth_Wheat = true }
for _, res in ipairs(game.assets.get_resources_by_type("keen::ecs::TemplateResource")) do
	local name = tostring(res.data.name)
	for _, c in ipairs(res.data.components) do
		if tostring(c.type) == "keen::ecs::Upgrade" and want[name] then
			local levels = {}
			local okl, err = pcall(function() for i, lv in pairs(c.value.levels) do levels[#levels + 1] = { tonumber(i) or #levels + 1, lv } end end)
			if not okl then p("STAGEERR " .. tostring(err)) end
			table.sort(levels, function(a, b) return a[1] < b[1] end)
			local last
			for _, entry in ipairs(levels) do
				local g = tostring(entry[2].upgradeEntity)
				local t = byguid[g]
				p(("STAGE %s %d %s %s"):format(name, entry[1], g, t and tostring(t.data.name) or "?"))
				last = t or last
			end
			if last then
				for _, lc in ipairs(last.data.components) do
					local t = tostring(lc.type)
					if not skip[t] then p("  LAST " .. tostring(last.data.name) .. " " .. t .. " " .. dump(lc.value)) end
				end
			end
		end
	end
end

-- fishing spots and the spawn table setups they name
for _, res in ipairs(game.assets.get_resources_by_type("keen::ecs::TemplateResource")) do
	for _, c in ipairs(res.data.components) do
		local t = tostring(c.type)
		if t == "keen::ecs::FishingSpot" or t == "keen::ecs::Fishing" then
			p(("SPOT %s %s %s"):format(tostring(res.data.name), t, dump(c.value)))
		end
	end
end
local okset, sets = pcall(game.assets.get_resources_by_type, "keen::fishing::FishSpawnTableSetup")
p("# setups " .. tostring(okset and #sets or sets))
if okset then for _, res in ipairs(sets) do p("SETUP " .. tostring(res.guid) .. " " .. dump(res.data)) end end
-- seedlings -> the plant root they place, and each root's stages with their harvest
-- inventory (`DefaultInventory.defaultInventory`), to know a crop's seedling at runtime
local roots = {}
local stageof = {}
for _, res in ipairs(game.assets.get_resources_by_type("keen::ecs::TemplateResource")) do
	for _, c in ipairs(res.data.components) do
		if tostring(c.type) == "keen::ecs::Growth" then roots[tostring(res.guid)] = tostring(res.data.name) end
	end
end
for _, res in ipairs(game.assets.get_resources_by_type("keen::ecs::TemplateResource")) do
	if roots[tostring(res.guid)] then
		for _, c in ipairs(res.data.components) do
			if tostring(c.type) == "keen::ecs::Upgrade" then
				pcall(function()
					for i, lv in pairs(c.value.levels) do
						local g = tostring(lv.upgradeEntity)
						local t = byguid[g]
						local inv = ""
						if t then
							for _, sc in ipairs(t.data.components) do
								if tostring(sc.type) == "keen::ecs::DefaultInventory" then inv = tostring(sc.value.defaultInventory) end
							end
						end
						p(("ROOTSTAGE %s %s %s %s %s"):format(tostring(res.guid), roots[tostring(res.guid)], tostring(i), t and tostring(t.data.name) or g, inv))
					end
				end)
			end
		end
	end
end
for _, res in ipairs(game.assets.get_resources_by_type("keen::ItemInfo")) do
	local d = res.data
	local name = tostring(d.debugName)
	if name:find("Seedling") then
		local s = dump(d, 0)
		local found = {}
		for g in s:gmatch("%x%x%x%x%x%x%x%x%-%x%x%x%x%-%x%x%x%x%-%x%x%x%x%-%x%x%x%x%x%x%x%x%x%x%x%x") do
			if roots[g] and not found[g] then
				found[g] = true
				p(("SEEDROOT %s %s %s %s"):format(tostring(d.itemId.value), name, g, roots[g]))
			end
		end
	end
end
io.export("farm.txt", table.concat(out, "\n"))
