local ok, src = pcall(decompile, game:GetService("ReplicatedStorage").BrickBattle.Sword)
local lines = ok and string.split(src, "\n") or {}
local out = {"ok="..tostring(ok)}
for i = 360, 372 do out[#out+1] = i..": "..tostring(lines[i]) end
return table.concat(out, "\n")
