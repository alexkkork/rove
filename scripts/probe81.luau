local s = game:GetService("ReplicatedFirst").ClassicLoadingScreen.ClassicLoadingScreen
local ok, src = pcall(decompile, s)
if not ok then return "dec fail: "..tostring(src) end
local lines = string.split(src, "\n")
local out = {}
for i = 118, 140 do out[#out+1] = i..": "..tostring(lines[i]) end
return table.concat(out, "\n")
