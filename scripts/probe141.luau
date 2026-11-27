task.wait(6)
local bad = {}
for _, d in ipairs(workspace:GetDescendants()) do
  local n = d.Name
  if n:sub(1,10) == "$Attribute" then
    local ok, v = pcall(function() return d.Value end)
    if not ok or v == nil then
      bad[#bad+1] = d:GetFullName().." cls="..d.ClassName.." ok="..tostring(ok).." v="..tostring(v)
      if #bad > 5 then break end
    end
  end
end
return "bad="..#bad.."\n"..table.concat(bad, "\n")
