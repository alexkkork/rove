local mod = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
local p = Instance.new("Part")
mod.setAttribute(p, "IsLunging", true)
local c = p:FindFirstChild("$AttributeIsLunging")
local out = {"cls="..c.ClassName}
for i = 1, 4 do
  local r = table.pack(pcall(function() return c.Value end))
  out[#out+1] = i..":"..tostring(r[1]).." "..typeof(r[2]).."="..tostring(r[2])
end
local g = table.pack(pcall(function() return mod.getAttribute(p, "IsLunging") end))
out[#out+1] = "get="..tostring(g[1]).." "..typeof(g[2]).."="..tostring(g[2])
return table.concat(out, "\n")
