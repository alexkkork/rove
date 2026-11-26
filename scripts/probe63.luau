local s = game:GetService("ReplicatedFirst").ClassicLoadingScreen.ClassicLoadingScreen
local src = s.Source
if not src then return "no source" end
local lines = string.split(src, "\n")
local out = {}
for i = 120, 140 do out[#out+1] = i..": "..tostring(lines[i]) end
return table.concat(out, "\n")
