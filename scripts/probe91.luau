local m = game:GetService("ReplicatedStorage").BrickBattle.Attributes
local bad = {}
for _, d in ipairs(workspace:GetDescendants()) do
  local ok, attrs = pcall(function() return d:GetAttributes() end)
  if ok then
    for k in pairs(attrs) do
      local c = d:FindFirstChild("$Attribute"..k)
      if c and c:IsA("Vector3Value") then
        local v = c.Value
        if v == nil then bad[#bad+1] = d:GetFullName().."."..k end
      end
    end
  end
end
return "bad="..#bad.."\n"..table.concat(bad, "\n")
