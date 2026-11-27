local c = game:GetService("ReplicatedStorage").Database.Sync.Item
local s = c.Source
return "len="..#s.." head="..s:sub(1,80):gsub("\n","|")
