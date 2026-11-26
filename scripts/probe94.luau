task.wait(6)
local bad = 0
local n = 0
local samples = {}
for _, d in ipairs(workspace:GetDescendants()) do
  if d:IsA("BasePart") then
    n += 1
    local ok, v = pcall(function() return d:GetAttribute("IsLunging") end)
    if not ok and #samples < 3 then
      local attrs = {}
      for k in pairs(d:GetAttributes()) do attrs[#attrs+1] = k end
      samples[#samples+1] = d:GetFullName().." attrs="..table.concat(attrs, ",")
    end
    if not ok then bad += 1 end
  end
end
return "n="..n.." bad="..bad.."\n"..table.concat(samples, "\n")
