local ok, src = pcall(decompile, game:GetService("ReplicatedStorage").BrickBattle.Sword)
local lines = ok and string.split(src, "\n") or {}
local out = {}
for i = 108, 130 do out[#out+1] = i..": "..tostring(lines[i]) end
return table.concat(out, "\n")
