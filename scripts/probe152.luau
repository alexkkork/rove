local c = game:GetService("ReplicatedStorage").Database.Sync.Item
local ok, bc = pcall(getscriptbytecode, c)
local ok2, h = pcall(getscripthash, c)
local out = {"bc="..tostring(ok).." len="..tostring(ok and #bc), "hash="..tostring(ok2).." "..tostring(h):sub(1,40)}
local s = c.Source
out[#out+1] = "src="..#s.." head="..s:sub(1, 60)
return table.concat(out, "\n")
