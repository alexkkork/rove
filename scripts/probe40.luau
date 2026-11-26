local mod = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
task.wait(6)
local lp = game:GetService("Players").LocalPlayer
local function findTool()
  local c = lp.Character
  local t = c and c:FindFirstChildWhichIsA("Tool")
  if t then return t end
  local bp = lp:FindFirstChild("Backpack")
  return bp and bp:FindFirstChildWhichIsA("Tool")
end
local tool = findTool()
if not tool then return "no tool" end
local c = tool:FindFirstChild("$AttributeIsLunging")
if not c then return "child missing: "..tool.Name end
local r = table.pack(pcall(function() return c.Value end))
return "cls="..c.ClassName.." read="..tostring(r[1]).." "..typeof(r[2]).."="..tostring(r[2])
