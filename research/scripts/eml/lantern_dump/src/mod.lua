-- Research: the buff behind the Ring of Light, and buffs that carry VFX.
if not loader.features.export then
	return
end

local out = {}
local function p(s) out[#out + 1] = s end

local function dump(v, indent, depth)
	indent = indent or ""
	depth = depth or 0
	if depth > 8 then return "..." end
	if type(v) ~= "table" and type(v) ~= "userdata" then return tostring(v) end
	local parts = {}
	local ok = pcall(function()
		for k, x in pairs(v) do
			parts[#parts + 1] = indent .. "  " .. tostring(k) .. " = " .. dump(x, indent .. "  ", depth + 1)
		end
	end)
	if not ok or #parts == 0 then
		local ok2, t = pcall(function() return v.type end)
		local ok3, val = pcall(function() return v.value end)
		if ok2 and ok3 then return "<" .. tostring(t) .. "> " .. dump(val, indent, depth + 1) end
		return tostring(v)
	end
	return "{\n" .. table.concat(parts, "\n") .. "\n" .. indent .. "}"
end

for _, res in ipairs(game.assets.get_resources_by_type("keen::ecs::TemplateResource")) do
	if tostring(res.guid) == "1890618a-992e-46b1-ad68-51bd8bf5e3a0" then
		for _, c in ipairs(res.data.components) do
			if tostring(c.type) == "keen::ecs::ImpactConfiguration" then
				p("ring visual config " .. dump(c.value.configValues))
			end
		end
	end
end

local ring_buff = "b2f8db72-a69d-43db-baf1-c69937ec0264"
local ok, h = pcall(game.guid.hash, ring_buff)
p("ring buff hash " .. tostring(ok) .. " " .. tostring(h))
for _, res in ipairs(game.assets.get_resources_by_type("keen::BuffTypeCollectionResource")) do
	for _, b in ipairs(res.data.buffs) do
		local vfx = #b.vfxResourceReferences.meshes + #b.vfxResourceReferences.models
		local line = ("BUFF id=%s slot=%s apply=%s impact=%s vfx=%d hideInUi=%s"):format(tostring(b.buffTypeId.value), tostring(b.slot), tostring(b.applyType), tostring(b.impact), vfx, tostring(b.hideInUi))
		if ok and b.buffTypeId.value == h then
			p("RING " .. line .. " " .. dump(b))
		end
		p(line)
	end
end
io.export("lantern5.txt", table.concat(out, "\n"))
