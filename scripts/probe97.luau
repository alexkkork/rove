task.wait(6)
local bad = {}
for _, d in ipairs(workspace:GetDescendants()) do
  if d:IsA("BasePart") then
    local ok, kids = pcall(function() return d:GetChildren() end)
    if ok then
      for _, c in ipairs(kids) do
        if c.Name:sub(1,10) == "$Attribute" then
          local ok2, v = pcall(function() return c.Value end)
          if not ok2 then bad[#bad+1] = c:GetFullName().." "..tostring(v):sub(1,70) end
        end
      end
    end
  end
end
return "bad="..#bad.."\n"..table.concat(bad, "\n")
