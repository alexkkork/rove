local out = {}
for _, d in ipairs(workspace:GetDescendants()) do
  if d:IsA("Animation") then
    local ok, c = pcall(function() return d.AnimationContent end)
    out[#out+1] = d.Name.." content_ok="..tostring(ok).." "..tostring(c)
    if #out > 4 then break end
  end
end
return table.concat(out, "\n")
