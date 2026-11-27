task.wait(3)
local d = game:GetService("ReplicatedStorage").Database.Sync
local c = d:FindFirstChild("Emotes")
local out = {"c="..tostring(c)}
if c then
  out[#out+1] = "cls="..c.ClassName
  local ok, s = pcall(function() return c.Source end)
  out[#out+1] = "src ok="..tostring(ok).." len="..tostring(ok and #s or s)
  out[#out+1] = "parent="..tostring(c.Parent)
  local ok2, e = pcall(require, c)
  out[#out+1] = "req="..tostring(ok2).." "..tostring(e):sub(1,180)
end
return table.concat(out, "\n")
