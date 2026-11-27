local ok, src = pcall(decompile, game:GetService("ReplicatedStorage").ClassicOverlays.Leaderboard.Leaderboard)
local lines = string.split(src, "\n")
return table.concat({for i = 496, 512 do i.."="..tostring(lines[i]) end}, "\n")
