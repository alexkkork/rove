local ok, src = pcall(decompile, game:GetService("ReplicatedStorage").ClassicOverlays.Leaderboard.Leaderboard)
local lines = string.split(src, "\n")
local out = {}
for i = 491, 504 do out[#out+1] = i..": "..tostring(lines[i]) end
return table.concat(out, "\n")
