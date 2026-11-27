local d = game:GetService("ReplicatedStorage").Database.Sync
local c = d:FindFirstChild("Emotes")
local ok, err = pcall(require, c)
return "req="..tostring(ok).." "..tostring(err):sub(1,200)
