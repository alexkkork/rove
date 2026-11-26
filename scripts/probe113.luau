local pg = game:GetService("Players").LocalPlayer:FindFirstChild("PlayerGui")
local names = {}
for _, d in ipairs(pg:GetDescendants()) do
  local ok, y = pcall(function() return d.AbsoluteSize.Y end)
  if not ok or y == nil then names[#names+1] = d:GetFullName().." ok="..tostring(ok).." y="..tostring(y) end
end
return "n="..#names.."\n"..table.concat(names, "\n")
