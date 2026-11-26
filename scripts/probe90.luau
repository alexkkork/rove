task.wait(3)
local bad, ok, n = 0, 0, 0
local sample
for _, d in ipairs(workspace:GetDescendants()) do
  if d:IsA("BasePart") then
    n += 1
    local a = pcall(function() return d.Position end)
    local b = pcall(function() return d.Size.Magnitude end)
    if a and b then ok += 1 else bad += 1; sample = sample or (d:GetFullName().." pos="..tostring(a).." size="..tostring(b)) end
  end
end
return "n="..n.." ok="..ok.." bad="..bad.." "..tostring(sample)
