local mod = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
local p = Instance.new("Part")
local out = {}
local function kids()
  local t = {}
  for _, c in ipairs(p:GetChildren()) do t[#t+1] = c.Name..":"..c.ClassName end
  return table.concat(t, ",")
end
out[#out+1] = "before="..kids()
local r1 = table.pack(pcall(mod.setAttribute, p, "IsLunging", true))
out[#out+1] = "set="..tostring(r1[1]).." "..tostring(r1[2]):sub(1,100)
out[#out+1] = "after="..kids()
local r2 = table.pack(pcall(mod.getAttribute, p, "IsLunging"))
out[#out+1] = "get="..tostring(r2[1]).." "..tostring(r2[2])
return table.concat(out, "\n")
