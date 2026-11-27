local c = game:GetService("ReplicatedStorage").Database.Sync.Codes
local s = c.Source
local hex = ""
for i = 1, 24 do hex ..= string.format("%02x ", string.byte(s, i)) end
return "len="..#s.." hex="..hex.." printable="..(s:gsub("[^%g ]","."):sub(1,60))
