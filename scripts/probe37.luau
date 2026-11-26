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
local out = {"tool="..tool.Name}
for _, k in ipairs({"$AttributeIsLunging","$AttributeLungeStartedAt"}) do
  local c = tool:FindFirstChild(k)
  if c then
    local r = table.pack(pcall(function() return c.Value end))
    out[#out+1] = k.." cls="..c.ClassName.." read="..tostring(r[1]).." "..tostring(r[2])
  else out[#out+1] = k.." missing" end
end
return table.concat(out, "\n")
