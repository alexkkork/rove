task.wait(6)
local mod = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
local n, bad = 0, 0
local sample
for _, d in ipairs(workspace:GetDescendants()) do
  if d:IsA("BasePart") then
    n += 1
    local ok, v = pcall(function() return mod.getAttribute(d, "IsLunging") end)
    if not ok then bad += 1; sample = sample or (d:GetFullName().." "..tostring(v):sub(1,90)) end
  end
end
return "n="..n.." bad="..bad.." "..tostring(sample)
