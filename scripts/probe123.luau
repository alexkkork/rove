local d = game:GetService("ReplicatedStorage").Database.Sync
local out = {"src="..#(d.Source or "")}
local kids = {}
for _, c in ipairs(d:GetChildren()) do kids[#kids+1] = c.Name..":"..c.ClassName.." src="..tostring(c.Source and #c.Source) end
out[#out+1] = "kids="..table.concat(kids, ",")
local ok, v = pcall(getscriptbytecode, d)
out[#out+1] = "bc="..tostring(ok).." "..tostring(v and #v)
return table.concat(out, "\n")
