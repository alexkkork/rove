local s = game:GetService("Players").LocalPlayer.PlayerScripts.InteractiveScript
local ok, src = pcall(decompile, s)
if not ok then return tostring(src) end
local lines = string.split(src, "\n")
local out = {"n="..#lines}
for i = 1, 8 do out[#out+1] = i..": "..tostring(lines[i]) end
return table.concat(out, "\n")
