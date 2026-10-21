-- Bigger Storage: multiplies the slots of player-built storage chests.

local MULTIPLIER = 2
-- an inventory links at most 16 pages of 8 slots
local MAX_SLOTS = 128

-- player-buildable storage templates (Prop_Decoration_T0_Storage_16 ... T6_Storage_72_Magic)
local function is_player_storage(name)
	return name:match("^Prop_Decoration_T%d_Storage_") ~= nil
		or name == "Base_Chest_Player_Storage"
end

local function scaled(slots)
	return math.min(slots * MULTIPLIER, MAX_SLOTS)
end

local patched = {}

for _, res in ipairs(game.assets.get_resources_by_type("keen::ecs::TemplateResource")) do
	---@type keen.ecs.TemplateResource
	local template = res.data

	if is_player_storage(template.name) then
		local before, after
		for _, component in ipairs(template.components) do
			if component.type == "keen::ecs::InventorySetup" then
				before = component.value.genericSlotCount
				after = scaled(before)
				component.value.genericSlotCount = after
				component.value.availableSlotCount = scaled(component.value.availableSlotCount)
			elseif component.type == "keen::ecs::InventorySetupOverwrite" then
				component.value.genericSlotCount = scaled(component.value.genericSlotCount)
			end
		end
		if before then
			patched[tostring(res.guid)] = after
			print(("%s: %d -> %d slots"):format(template.name, before, after))
		end
	end
end

-- the UI reads the slot count of templates from the template info collection
for _, res in ipairs(game.assets.get_resources_by_type("keen::TemplateInfoCollection")) do
	---@type keen.TemplateInfoCollection
	local collection = res.data
	for i, guid in ipairs(collection.templateGuids) do
		local slots = patched[tostring(guid)]
		if slots then
			local info = collection.templateInfos[i]
			if info.containerSlotCount ~= 0 then
				info.containerSlotCount = slots
			end
			if info.inventorySlotCount ~= 0 then
				info.inventorySlotCount = slots
			end
		end
	end
end
