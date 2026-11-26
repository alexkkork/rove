local out = {}
for _, d in ipairs(game:GetService("ReplicatedStorage"):GetDescendants()) do
  if d:IsA("Animation") then
    out[#out+1] = d.Name.." id='"..tostring(d.AnimationId).."' content="..tostring((pcall(function() return d.AnimationContent end)))
    if #out > 5 then break end
  end
end
return table.concat(out, "\n")
