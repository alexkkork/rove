local mod = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
local sword = game:GetService("ReplicatedStorage").BrickBattle.Sword
local out = {}
local function kids()
  local t = {}
  for _, c in ipairs(sword:GetChildren()) do t[#t+1] = c.Name end
  return table.concat(t, ",")
end
local r1 = table.pack(pcall(mod.setAttribute, sword, "IsLunging", true))
out[#out+1] = "set="..tostring(r1[1]).." "..tostring(r1[2]):sub(1,150)
out[#out+1] = "kids="..kids()
local r2 = table.pack(pcall(mod.getAttribute, sword, "IsLunging"))
out[#out+1] = "get="..tostring(r2[1]).." "..tostring(r2[2]):sub(1,150)
-- manual reproduction
local x = Instance.new("ObjectValue"); x.Name="probe"; x.Parent = sword
out[#out+1] = "manualkids="..kids()
return table.concat(out, "\n")
