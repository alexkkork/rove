local s = game:GetService("ReplicatedFirst").ClassicLoadingScreen.ClassicLoadingScreen
local ok, d = pcall(decompile, s)
if not ok then return tostring(d) end
local lines = string.split(d, "\n")
local out = {}
for i = 124, 136 do out[#out+1] = i..": "..tostring(lines[i]) end
return table.concat(out, "\n")
