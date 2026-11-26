local mod = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
local bp = game:GetService("Players").LocalPlayer:FindFirstChild("Backpack")
local tool = bp and bp:FindFirstChildWhichIsA("Tool")
if not tool then return "no tool" end
local out = { "tool="..tool.Name }
for k, v in pairs(tool:GetAttributes()) do
  local r = table.pack(pcall(function() return mod.getAttribute(tool, k) end))
  out[#out+1] = k.." -> "..tostring(r[1]).." "..typeof(r[2])..":"..tostring(r[2])
end
-- GridPosition on humanoid
local hum = game:GetService("Players").LocalPlayer.Character:FindFirstChildOfClass("Humanoid")
local r2 = table.pack(pcall(function() return mod.getAttribute(hum, "GridPosition") end))
out[#out+1] = "grid -> "..tostring(r2[1]).." "..typeof(r2[2])..":"..tostring(r2[2])
return table.concat(out, "\n")
