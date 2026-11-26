local s = game:GetService("ReplicatedFirst").ClassicLoadingScreen.ClassicLoadingScreen
local src = s.Source
local lines = string.split(src, "\n")
return "n="..#lines.." \n"..table.concat(lines, "\n")
