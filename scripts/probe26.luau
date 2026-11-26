local mod = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
local lp = game:GetService("Players").LocalPlayer
local bp = lp:FindFirstChild("Backpack")
local tool = bp and bp:FindFirstChildWhichIsA("Tool")
local out = {"tool="..tostring(tool)}
if tool then
  local attrs = tool:GetAttributes()
  for k, v in pairs(attrs) do
    out[#out+1] = k.." | direct="..tostring(tool:GetAttribute(k)).." mod="..tostring(mod.getAttribute(tool,k))
  end
end
return table.concat(out, "\n")
