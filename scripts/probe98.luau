-- exact getAttribute path from the game module
local mod = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
local bad = {}
for _, d in ipairs(workspace:GetDescendants()) do
  if d:IsA("BasePart") then
    for _, c in ipairs(d:GetChildren()) do
      if c.Name:sub(1,10) == "$Attribute" then
        local name = c.Name:sub(11)
        local ok, v = pcall(function() return mod.getAttribute(d, name) end)
        if not ok then bad[#bad+1] = d:GetFullName().."."..name.." -> "..tostring(v):sub(1,80) end
      end
    end
  end
end
return "bad="..#bad.."\n"..table.concat(bad, "\n")
