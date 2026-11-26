local s = game:GetService("ReplicatedStorage").Database.Sync
local out = {}
local ok, v = pcall(getscriptbytecode, s)
out[#out+1] = "bc ok="..tostring(ok).." "..tostring(v and #v)
local ok2, err = pcall(require, s)
out[#out+1] = "req ok="..tostring(ok2).." "..tostring(err):sub(1,200)
return table.concat(out, "\n")
