local c = game:GetService("ReplicatedStorage").Database.Sync.Item
local ok, src = pcall(decompile, c)
local out = {"dec="..tostring(ok).." "..tostring(src):sub(1,120)}
local co = coroutine.create(function() return require(c) end)
local r1, r2 = coroutine.resume(co)
out[#out+1] = "req="..tostring(r1).." "..tostring(r2):sub(1,150)
return table.concat(out, "\n")
