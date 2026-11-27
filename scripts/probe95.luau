task.wait(5)
local mod = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
local bad, names = 0, {}
for _, d in ipairs(workspace:GetDescendants()) do
  if d:IsA("BasePart") then
    local ok = pcall(function() return mod.getAttribute(d, "IsLunging") end)
    if not ok then bad += 1; if #names < 4 then names[#names+1] = d:GetFullName() end end
  end
end
return "bad="..bad.." "..table.concat(names, " | ")
