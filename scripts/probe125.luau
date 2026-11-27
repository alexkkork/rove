local d = game:GetService("ReplicatedStorage").Database.Sync
local ok, src = pcall(decompile, d)
if not ok then return "dec:"..tostring(src) end
local lines = string.split(src, "\n")
local out = {"n="..#lines}
for i = 1, 10 do out[#out+1] = i..": "..tostring(lines[i]) end
return table.concat(out, "\n")
