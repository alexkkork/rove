local mod = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
local lp = game:GetService("Players").LocalPlayer
local bp = lp:FindFirstChild("Backpack")
if not bp then return "no backpack" end
local tool
for _, c in ipairs(bp:GetChildren()) do
  if string.find(c.Name, "Sword") then tool = c break end
end
if not tool then
  tool = bp:FindFirstChildWhichIsA("Tool")
end
local out = {"tool="..tostring(tool and tool.Name)}
if not tool then return table.concat(out,"\n") end
local kids = {}
for _, c in ipairs(tool:GetChildren()) do kids[#kids+1] = c.Name..":"..c.ClassName.."="..tostring(c.Value) end
out[#out+1] = "kids="..table.concat(kids, ",")
for k, v in pairs(tool:GetAttributes()) do out[#out+1] = "attr "..k.."="..typeof(v) end
local r = table.pack(pcall(function() return mod.getAttribute(tool, "IsLunging") end))
out[#out+1] = "call="..tostring(r[1]).." "..tostring(r[2]):sub(1,200)
return table.concat(out, "\n")
