local ok, src = pcall(decompile, game:GetService("ReplicatedStorage").ClassicOverlays.Leaderboard.Leaderboard)
if not ok then return tostring(src) end
local lines = string.split(src, "\n")
local out = {}
for i = 492, 506 do out[#out+1] = i..": "..tostring(lines[i]) end
return table.concat(out, "\n")
