local mod = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
local lp = game:GetService("Players").LocalPlayer
local char = lp.Character or lp.CharacterAdded:Wait()
local hum = char:WaitForChild("Humanoid")
local out = {}
local function t(label, f)
  local r = table.pack(pcall(f))
  out[#out+1] = label.." -> "..tostring(r[1]).." "..typeof(r[2])..":"..tostring(r[2]):sub(1,90)
end
t("grid", function() return mod.getAttribute(hum, "GridPosition") end)
t("missing", function() return mod.getAttribute(hum, "Nope") end)
t("missingPart", function() return mod.getAttribute(char:FindFirstChild("Head"), "Nope") end)
local bp = lp:FindFirstChild("Backpack")
local tool = bp and bp:FindFirstChildWhichIsA("Tool")
t("toolAttr", function() return mod.getAttribute(tool, "Input_BaseDamage") end)
t("toolMissing", function() return mod.getAttribute(tool, "Nope") end)
t("toolNil", function() return mod.getAttribute(tool, nil) end)
return table.concat(out, "\n")
