-- Research: the build zone of a base per flame level (`BalancingTable.buildzoneSizesPerAltarLevel`,
-- a box per level). See docs/research/assistants.md.
if not loader.features.export then return end
local out = {}
for _, res in ipairs(game.assets.get_resources_by_type("keen::BalancingTable")) do
	local ok, err = pcall(function()
		for i, v in ipairs(res.data.buildzoneSizesPerAltarLevel) do
			local function n(x) local okv, val = pcall(function() return x.value end) return tostring(okv and val or x) end
			out[#out + 1] = ("BUILDZONE %s level %d %s %s %s"):format(tostring(res.guid), i - 1, n(v.x), n(v.y), n(v.z))
		end
	end)
	if not ok then out[#out + 1] = "ERROR " .. tostring(err) end
end
io.export("base.txt", table.concat(out, "\n"))
