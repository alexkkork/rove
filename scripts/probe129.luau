local lp = game:GetService("Players").LocalPlayer
for i=1,60 do if lp.Character then break end task.wait(.5) end
task.wait(8)
local d = game:GetService("ReplicatedStorage").Database.Sync
local c = d:FindFirstChild("Emotes")
local ok, err = pcall(require, c)
return "req="..tostring(ok).." "..tostring(err):sub(1,250)
