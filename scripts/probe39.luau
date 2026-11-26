local mod = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
local p = Instance.new("Part")
local out = {}
for _, case in ipairs({{"B", true}, {"N", 5}, {"S", "x"}, {"V", Vector3.new(1,2,3)}, {"C", CFrame.new(0,0,0)}}) do
  local ok = pcall(mod.setAttribute, p, case[1], case[2])
  local child = p:FindFirstChild("$Attribute"..case[1])
  out[#out+1] = case[1].." set="..tostring(ok).." child="..tostring(child and child.ClassName)
end
return table.concat(out, "\n")
