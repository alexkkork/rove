local ok, src = pcall(decompile, game:GetService("Players").LocalPlayer.PlayerGui.GameTopbar.TopBarScript)
if not ok then return tostring(src) end
local lines = string.split(src, "\n")
local out = {}
for i = 44, 56 do out[#out+1] = i..": "..tostring(lines[i]) end
return table.concat(out, "\n")
