task.wait(5)
local n, bad = 0, 0
local sample
for _, d in ipairs(workspace:GetDescendants()) do
  if d:IsA("BasePart") then
    n += 1
    local ok, size = pcall(function() return d.Size end)
    if not ok then bad += 1; sample = sample or (d:GetFullName().." "..tostring(size):sub(1,80)) end
  end
end
return "n="..n.." bad="..bad.." "..tostring(sample)
