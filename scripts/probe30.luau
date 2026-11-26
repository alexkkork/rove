local mod = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
local sword = game:GetService("ReplicatedStorage").BrickBattle.Sword
local out = {"class="..sword.ClassName}
local attrs = sword:GetAttributes()
for k, v in pairs(attrs) do out[#out+1] = "attr "..k.."="..typeof(v)..":"..tostring(v) end
local kids = {}
for _, c in ipairs(sword:GetChildren()) do kids[#kids+1] = c.Name..":"..c.ClassName end
out[#out+1] = "kids="..table.concat(kids, ",")
local r = table.pack(pcall(function() return mod.getAttribute(sword, "IsLunging") end))
out[#out+1] = "call="..tostring(r[1]).." "..tostring(r[2]):sub(1,160)
return table.concat(out, "\n")
