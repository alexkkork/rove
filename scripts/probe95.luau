local m = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
local p = Instance.new("Part")
m.setAttribute(p, "Test", Vector3.new(1,2,3))
local c = p:FindFirstChild("$AttributeTest")
local out = {"child="..tostring(c)}
if c then
  out[#out+1] = "cls="..c.ClassName
  local r = table.pack(pcall(function() return c.Value end))
  out[#out+1] = "read="..tostring(r[1]).." "..typeof(r[2])..":"..tostring(r[2])
  out[#out+1] = "props="..table.concat((function() local t={} for k in pairs(c:GetAttributes()) do t[#t+1]=k end return t end)(), ",")
end
return table.concat(out, "\n")
