-- Research: the Gemforge players build in their base (Prop_NPC_T7_Player_GemForge): its
-- template and the items around gems, to see why it takes ingredients from the backpack only.
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
for _, res in ipairs(game.assets.get_resources_by_type("keen::ecs::TemplateResource")) do
	local name = tostring(res.data.name)
	if name:find("GemForge") or name:find("Gemforge") or name:find("Gem_Forge") or name:find("Smelter") or name:find("Storage_.*Magic") then
		p(("TEMPLATE %s %s"):format(name, tostring(res.guid)))
		for _, c in ipairs(res.data.components) do
			local t = tostring(c.type)
			if not skip[t] then p("  " .. t .. " " .. dump(c.value)) end
		end
	end
end
for _, res in ipairs(game.assets.get_resources_by_type("keen::ItemInfo")) do
	local d = res.data
	local name = tostring(d.debugName)
	if name:find("Gem") or name:find("Spider") or name:find("Arach") then
		p(("ITEM %s %s %s"):format(tostring(d.itemId.value), name, dump(d, 3)))
	end
end
-- what ShroudForge.upgradeMaterial (a guid) names
local target = "bd3e92fc-a65e-4838-a024-09f382de2a48"
for _, res in ipairs(game.assets.get_resources_by_type("keen::ItemInfo")) do
	if tostring(res.guid) == target then p(("UPGRADEITEM %s %s"):format(tostring(res.data.itemId.value), tostring(res.data.debugName))) end
end
for _, ty in ipairs({ "keen::ShroudForgeResource", "keen::ShroudForgeUpgradeMaterialResource", "keen::GemUpgradeMaterialResource", "keen::ItemUpgradeMaterialResource", "keen::UpgradeMaterialResource", "keen::ShroudForgeUpgradeResource", "keen::GemSettingsResource", "keen::GemResource" }) do
	local ok, list = pcall(game.assets.get_resources_by_type, ty)
	p("# " .. ty .. " " .. tostring(ok and #list or list))
	if ok then
		for _, res in ipairs(list) do p("RES " .. ty .. " " .. tostring(res.guid) .. " " .. dump(res.data, 0)) end
	end
end
io.export("gemforge.txt", table.concat(out, "\n"))
