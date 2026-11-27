task.wait(4)
local d = game:GetService("ReplicatedStorage").Database.Sync
local c = d:FindFirstChild("Item")
local out = {"item="..tostring(c)}
if c then
  out[#out+1] = "cls="..c.ClassName.." src="..tostring(c.Source and #c.Source)
  local co = coroutine.create(function() return require(c) end)
  local ok, v = coroutine.resume(co)
  out[#out+1] = "req="..tostring(ok).." "..tostring(v):sub(1,200)
end
return table.concat(out, "\n")
