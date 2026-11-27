local c = game:GetService("ReplicatedStorage").Database.Sync.Item
local out = {}
local ok, bc = pcall(getscriptbytecode, c)
out[#out+1] = "bc="..tostring(ok).." "..tostring(bc and #bc)
local co = coroutine.create(function() return require(c) end)
local r1, r2 = coroutine.resume(co)
out[#out+1] = "req="..tostring(r1).." "..tostring(r2):sub(1,160)
return table.concat(out, "\n")
