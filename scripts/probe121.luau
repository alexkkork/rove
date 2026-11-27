local out = {}
local bounds = 0
for _, d in ipairs(workspace:GetDescendants()) do
  if d:IsA("InputBinding") then
    bounds += 1
    if bounds <= 3 then
      local kid = {}
      for k, v in pairs(d:GetAttributes()) do kid[#kid+1] = k.."="..typeof(v) end
      out[#out+1] = d:GetFullName().." kc="..typeof(d.KeyCode)..":"..tostring(d.KeyCode).." attrs="..table.concat(kid, ",")
    end
  end
end
return "n="..bounds.."\n"..table.concat(out, "\n")
