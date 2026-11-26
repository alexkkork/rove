local found
for _, d in ipairs(workspace:GetDescendants()) do
  if d:IsA("Animation") then found = d break end
end
local id = found and found.AnimationId
return "id="..tostring(id)
