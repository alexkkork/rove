local m = game:GetService("ReplicatedStorage").BrickBattle.Attributes
local src = m.Source
local lines = string.split(src, "\n")
local out = {}
for i = 30, 60 do out[#out+1] = i..": "..tostring(lines[i]) end
return table.concat(out, "\n")
