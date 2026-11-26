local s = game:GetService("ReplicatedFirst").ClassicLoadingScreen.ClassicLoadingScreen
local ok, src = pcall(decompile, s)
local lines = string.split(src, "\n")
local out = {}
for i = 140, 200 do out[#out+1] = i..": "..tostring(lines[i]) end
return table.concat(out, "\n")
