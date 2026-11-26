local m = game:GetService("ReplicatedStorage").BrickBattle.Attributes
local holder = game:GetService("ReplicatedStorage").__attrprobe
local p = holder:FindFirstChild("P")
m.setAttribute(p, "Vec", Vector2.new(3,4))
local out = {}
for _, c in ipairs(p:GetChildren()) do out[#out+1] = c.Name..":"..c.ClassName end
for _, c in ipairs(p:GetChildren()) do out[#out+1] = "v="..tostring(c.Value) end
return table.concat(out, "\n")
