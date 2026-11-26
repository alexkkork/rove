local mod = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
local out = {}
local function t(n, f)
  local r = table.pack(pcall(f))
  local s = n.."="..tostring(r[1])
  if not r[1] then s = s.." err="..tostring(r[2]):sub(1,120) else s=s.." val="..tostring(r[2]) end
  out[#out+1]=s
end
local p = Instance.new("Part")
t("fresh", function() return mod.getAttribute(p, "Nope") end)
p:SetAttribute("A", Vector2.new(3,4))
t("vec2", function() return mod.getAttribute(p, "A") end)
local d = Instance.new("Part"); d:SetAttribute("B", 5); d:Destroy()
t("destroyed", function() return mod.getAttribute(d, "B") end)
t("nilinst", function() return mod.getAttribute(nil, "B") end)
t("setattr", function() return mod.setAttribute(p, "C", 9) end)
t("bind", function() return mod.bindToAttributeChanged(p, "C", function() end) end)
return table.concat(out, "\n")
