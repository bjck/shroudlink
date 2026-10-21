-- Bigger Stacks: multiplies the max stack size of items that already stack.
-- Items with a max stack of 1 carry per-item state (durability, rarity, perks)
-- and are skipped.

local MULTIPLIER = 10
-- ItemInfo.maxStackSize is a u16
local MAX_STACK = 65535

local changed = 0

for _, res in ipairs(game.assets.get_resources_by_type("keen::ItemInfo")) do
	---@type keen.ItemInfo
	local item = res.data
	if item.maxStackSize > 1 then
		item.maxStackSize = math.min(item.maxStackSize * MULTIPLIER, MAX_STACK)
		changed = changed + 1
	end
end

print(("Bigger Stacks: multiplied the stack size of %d items by %d"):format(changed, MULTIPLIER))
