task.wait(7)
local lp = game:GetService("Players").LocalPlayer
local char = lp.Character
local mod = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
local out = {"char="..tostring(char)}
if char then
  for _, d in ipairs(char:GetDescendants()) do
    for _, c in ipairs(d:GetChildren()) do
      if c.Name:sub(1,10) == "$Attribute" then
        local ok, v = pcall(function() return mod.getAttribute(d, c.Name:sub(11)) end)
        out[#out+1] = d.Name.."."..c.Name:sub(11).."="..tostring(ok).." "..tostring(v):sub(1,50)
      end
    end
  end
end
return table.concat(out, "\n")
