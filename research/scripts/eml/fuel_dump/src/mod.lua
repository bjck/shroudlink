-- Research: how production stations get fuel and energy (Fuel, FuelSetup, EnergyProvider,
-- FuelCraftingStock, rotational energy) and which items burn (ItemFuelData).
-- See docs/research/assistants.md.
if not loader.features.export then return end
local out = {}
local function p(s) out[#out + 1] = s end

local function dump(v, depth)
	depth = depth or 0
	if depth > 5 then return "..." end
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

local interesting = {
	["keen::ecs::Fuel"] = true, ["keen::ecs::FuelSetup"] = true, ["keen::ecs::EnergyProvider"] = true,
	["keen::ecs::FuelCraftingStock"] = true, ["keen::ecs::RotationalEnergyConsumer"] = true,
	["keen::ecs::RotationalEnergySource"] = true, ["keen::ecs::FactoryStation"] = true,
	["keen::ecs::Crafting"] = true, ["keen::ecs::InventorySetup"] = true, ["keen::ecs::FuelPercentageInit"] = true,
	["keen::ecs::InventoryCraftingStock"] = true, ["keen::ecs::RequiredCraftingProp"] = true, ["keen::ecs::NetworkFuel"] = true,
}
for _, res in ipairs(game.assets.get_resources_by_type("keen::ecs::TemplateResource")) do
	local hit, lines = false, {}
	for _, c in ipairs(res.data.components) do
		local t = tostring(c.type)
		if t == "keen::ecs::Fuel" or t == "keen::ecs::EnergyProvider" or t == "keen::ecs::FuelCraftingStock" or t == "keen::ecs::FactoryStation" then hit = true end
		if interesting[t] then lines[#lines + 1] = "  " .. t .. " " .. dump(c.value) end
	end
	if hit then
		p(("TEMPLATE %s %s"):format(res.data.name, tostring(res.guid)))
		for _, l in ipairs(lines) do p(l) end
	end
end

for _, res in ipairs(game.assets.get_resources_by_type("keen::ItemInfo")) do
	local d = res.data
	local ok, fuel = pcall(function() return d.fuelSetup end)
	if not ok or fuel == nil then ok, fuel = pcall(function() return d.fuel end) end
	if ok and fuel ~= nil then
		local okset, isset = pcall(function() return fuel.isSet end)
		if okset and isset then
			local okid, id = pcall(function() return d.itemId.value end)
			local okname, name = pcall(function() return d.debugName end)
			p(("FUELITEM %s %s burn=%s energy=%s"):format(tostring(okid and id or "?"), tostring(okname and name or res.guid), dump(fuel.burnTime), dump(fuel.energyOutput)))
		end
	end
end
io.export("fuel.txt", table.concat(out, "\n"))
