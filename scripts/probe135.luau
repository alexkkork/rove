local ok, src = pcall(decompile, game:GetService("ReplicatedStorage").ClassicOverlays.Leaderboard.Leaderboard)
local lines = string.split(src, "\n")
local out = {"n="..#lines}
for i = 455, 470 do out[#out+1] = i..": "..tostring(lines[i]) end
return table.concat(out, "\n")
