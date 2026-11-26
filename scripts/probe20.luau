local mod = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
local holder = game:GetService("ReplicatedStorage"):FindFirstChild("__attrprobe")
if not holder then holder = Instance.new("Folder"); holder.Name="__attrprobe"; holder.Parent=game:GetService("ReplicatedStorage") end
local p = holder:FindFirstChild("P") or Instance.new("Part")
p.Name="P"; p.Parent=holder
mod.setAttribute(p, "Vec", Vector2.new(3,4))
mod.setAttribute(p, "Str", "hi")
mod.setAttribute(p, "Num", 7)
local out = {}
for _, c in ipairs(p:GetChildren()) do
  out[#out+1] = c.Name..":"..c.ClassName..":"..tostring(c.Value)
end
return table.concat(out, "\n")
