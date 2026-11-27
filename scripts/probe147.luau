local n, nilmag = 0, 0
local sample
for _, part in ipairs(workspace:GetDescendants()) do
  if part:IsA("BasePart") then
    n += 1
    local size = part.Size
    local ok, mag = pcall(function() return size.Magnitude end)
    if not ok or mag == nil then nilmag += 1; sample = sample or (part:GetFullName().." size="..tostring(size)) end
  end
end
return "n="..n.." nilmag="..nilmag.."\n"..tostring(sample)
