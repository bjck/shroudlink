-- Research: fast-travel destinations (spires, altars) and where the world places them.
if not loader.features.export then return end
local out = {}
local function p(s) out[#out + 1] = s end

local fast = {}
for _, res in ipairs(game.assets.get_resources_by_type("keen::MapMarkerRegistryResource")) do
	for _, m in ipairs(res.data.mapMarkers.values) do
		p(("MARKERTYPE id=%s fastTravel=%s custom=%s"):format(tostring(m.markerId.value), tostring(m.isFastTravelDestination), tostring(m.isCustomMarkerType)))
		if m.isFastTravelDestination then fast[m.markerId.value] = true end
	end
end

local templates = {}
for _, res in ipairs(game.assets.get_resources_by_type("keen::ecs::TemplateResource")) do
	for _, c in ipairs(res.data.components) do
		if tostring(c.type) == "keen::ecs::MapMarker" then
			local ok, h = pcall(game.guid.hash, c.value.markerType)
			local isFast = ok and fast[h]
			p(("MARKER %s %s type=%s hash=%s fast=%s"):format(res.data.name, tostring(res.guid), tostring(c.value.markerType), tostring(h), tostring(isFast)))
			local mt = tostring(c.value.markerType)
			if mt == "77997dbc-4c4c-4843-a616-107505e4512d" or mt == "2bad3206-cad7-469a-a358-64ea99181416" or mt == "5ebdad4b-08b6-4578-8d63-fe9917449eaf" then
				templates[tostring(res.guid)] = res.data.name
			end
		end
	end
end

local chunks = 0
for _, res in ipairs(game.assets.get_resources_by_type("keen::SceneEntityChunkResource")) do
	chunks = chunks + 1
	local c = res.data
	for i, t in ipairs(c.templates) do
		local name = templates[tostring(t)]
		if name then
			for _, e in ipairs(c.entities) do
				if e.templateIndex == i - 1 then
					local tr = e.transform
					local parts = {}
					pcall(function() for k, v in pairs(tr) do parts[#parts + 1] = tostring(k) .. "=" .. tostring(v) end end)
					p(("PLACED %s chunk=%s tr={%s}"):format(name, tostring(res.guid), table.concat(parts, " ")))
				end
			end
		end
	end
end
p("chunks " .. chunks)
for _, ty in ipairs({ "keen::SceneRuntimeContent", "keen::SceneContent" }) do
	local ok, list = pcall(game.assets.get_resources_by_type, ty)
	p(ty .. " " .. tostring(ok) .. " " .. tostring(ok and #list))
	if ok then
		for _, res in ipairs(list) do
			for _, s in ipairs(res.data.spawns) do
				local name = templates[tostring(s.templateReference)]
				if name then
					local pos = s.transform.position
					p(("SPAWN %s x=%s y=%s z=%s"):format(name, tostring(pos.x), tostring(pos.y), tostring(pos.z)))
				end
			end
		end
	end
end
io.export("travel.txt", table.concat(out, "\n"))
