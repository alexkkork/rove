local m = game:GetService("ReplicatedStorage").BrickBattle.Attributes
local ok, src = pcall(decompile, m)
if not ok then return tostring(src) end
local lines = string.split(src, "\n")
local out = {}
for i = 30, 62 do out[#out+1] = i..": "..tostring(lines[i]) end
return table.concat(out, "\n")
