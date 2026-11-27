local out = {"bad=0"}
for _, part in ipairs(workspace:GetDescendants()) do
  if part:IsA("BasePart") then
    local ok, mag = pcall(function() return part.Size.Magnitude end)
    if not ok or mag == nil then
      out[1] = "bad="..tostring(tonumber(out[1]:sub(5)) + 1)
      if #out < 6 then out[#out+1] = part:GetFullName().." "..tostring(mag) end
    end
  end
end
return table.concat(out, "\n")
