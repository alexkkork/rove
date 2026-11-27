local c = game:GetService("ReplicatedStorage").Database.Sync.Item
local ok, bc = pcall(getscriptbytecode, c)
return "bc="..tostring(ok).." "..tostring(bc and #bc).." srclen="..tostring(c.Source and #c.Source)
