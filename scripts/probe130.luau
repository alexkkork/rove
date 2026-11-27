task.wait(3)
local c = game:GetService("ReplicatedStorage").Database.Sync.Emotes
local co = coroutine.create(function() return require(c) end)
local ok, err = coroutine.resume(co)
return "direct="..tostring(ok).." "..tostring(err):sub(1,250)
