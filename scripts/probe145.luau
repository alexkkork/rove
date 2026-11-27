local out = {"bad=0"}
for _, part in ipairs(workspace:GetDescendants()) do
  if part:IsA("BasePart") then
    local ok, size = pcall(function() return part.Size end)
    local ok2, pos = pcall(function() return part.Position end)
    if not ok or not ok2 or size == nil or pos == nil then
      out[1] = "bad="..tostring(tonumber(out[1]:sub(5)) + 1)
      if #out < 6 then
        out[#out+1] = part:GetFullName().." cls="..part.ClassName.." size="..tostring(ok and size).." pos="..tostring(ok2 and pos)
      end
    end
  end
end
return table.concat(out, "\n")
