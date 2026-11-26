local mod = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
task.wait(4)
local n, nilv, errs = 0, 0, 0
local sample
for _, d in ipairs(workspace:GetDescendants()) do
  if d:IsA("Humanoid") then
    n += 1
    local r = table.pack(pcall(mod.getAttribute, d, "GridPosition"))
    if not r[1] then errs += 1; sample = sample or tostring(r[2]):sub(1,80)
    elseif r[2] == nil then nilv += 1; sample = sample or ("nil for "..d:GetFullName().." hasAttr="..tostring(d:GetAttribute("GridPosition")).." child="..tostring(d:FindFirstChild("$AttributeGridPosition"))) end
  end
end
return "n="..n.." nil="..nilv.." err="..errs.."\n"..tostring(sample)
