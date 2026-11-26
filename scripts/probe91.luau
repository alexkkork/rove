task.wait(4)
local n, withPos, withSize = 0, 0, 0
local first
for _, d in ipairs(workspace:GetDescendants()) do
  if d:IsA("BasePart") then
    n += 1
    if pcall(function() return d.Position.X end) then withPos += 1 end
    if pcall(function() return d.Size.X end) then withSize += 1 end
    first = first or d
  end
end
local cf = first and first.CFrame
return "n="..n.." pos="..withPos.." size="..withSize.." cf="..tostring(cf)
