local found
for _, d in ipairs(workspace:GetDescendants()) do
  if d:IsA("Animation") then found = d break end
end
local out = {"a="..tostring(found and found:GetFullName())}
if found then
  for _, k in ipairs({"AnimationId","AnimationContent"}) do
    local ok, v = pcall(function() return found[k] end)
    out[#out+1] = k.."="..tostring(ok).." "..tostring(v):sub(1,60)
  end
end
return table.concat(out, "\n")
