local d = game:GetService("ReplicatedStorage").Database.Sync
local ok, src = pcall(decompile, d)
local lines = ok and string.split(src, "\n") or {}
local out = {}
for i = 1, 14 do out[#out+1] = i..": "..tostring(lines[i]) end
return table.concat(out, "\n")
