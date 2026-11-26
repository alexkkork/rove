local mod = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
local p = Instance.new("Part")
local out = {}
local function t(n, f)
  local r = table.pack(pcall(f))
  out[#out+1] = n.."="..tostring(r[1]).." "..tostring(r[2]):sub(1,110)
end
t("vec1", function() return mod.setAttribute(p, "A", Vector2.new(1,2)) end)
t("vec2", function() return mod.setAttribute(p, "B", Vector3.new(1,2,3)) end)
t("readA", function() return mod.getAttribute(p, "A") end)
local c = p:FindFirstChild("$AttributeA")
local r = table.pack(pcall(function() return c.Value end))
out[#out+1] = "childA="..tostring(c and c.ClassName).." value="..tostring(r[1]).." "..tostring(r[2])
return table.concat(out, "\n")
