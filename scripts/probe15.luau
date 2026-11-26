local mod = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
local out = {"type="..typeof(mod)}
for k, v in pairs(mod) do out[#out+1] = tostring(k).."="..typeof(v) end
return table.concat(out, "\n")
