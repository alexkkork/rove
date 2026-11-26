local m = game:GetService("ReplicatedStorage").BrickBattle.Attributes
local p = Instance.new("Part")
m.setAttribute(p, "Vec", Vector2.new(3,4))
m.setAttribute(p, "Str", "hi")
m.setAttribute(p, "Bol", true)
local out = {}
for _, c in ipairs(p:GetChildren()) do out[#out+1] = c.Name..":"..c.ClassName.."|"..tostring(c.Value) end
out[#out+1] = "getVec="..tostring(m.getAttribute(p,"Vec"))
return table.concat(out, "\n")
