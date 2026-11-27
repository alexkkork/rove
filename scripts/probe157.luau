local c = game:GetService("ReplicatedStorage").Database.Sync.Item
local out = {"full="..c:GetFullName()}
local ok, bc = pcall(getscriptbytecode, c)
out[#out+1] = "bc="..tostring(ok).." "..tostring(bc and #bc)
out[#out+1] = "srclen="..tostring(c.Source and #(c.Source))
return table.concat(out, "\n")
