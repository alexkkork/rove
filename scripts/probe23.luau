local m = game:GetService("ReplicatedStorage").BrickBattle.Attributes
local out = {}
local function t(n, f)
  local r = table.pack(pcall(f))
  out[#out+1] = n.."="..tostring(r[1]).." "..tostring(r[2]):sub(1,110)
end
local p = Instance.new("Part")
t("num", function() return m.setAttribute(p, "N", 5) end)
t("str", function() return m.setAttribute(p, "S", "hi") end)
t("bool", function() return m.setAttribute(p, "B", true) end)
t("vec2", function() return m.setAttribute(p, "V2", Vector2.new(1,2)) end)
t("vec3", function() return m.setAttribute(p, "V3", Vector3.new(1,2,3)) end)
t("cf", function() return m.setAttribute(p, "C", CFrame.new(1,2,3)) end)
t("color", function() return m.setAttribute(p, "Col", Color3.new(1,0,0)) end)
local kids = {}
for _, c in ipairs(p:GetChildren()) do kids[#kids+1] = c.Name end
out[#out+1] = "kids="..table.concat(kids, ",")
return table.concat(out, "\n")
