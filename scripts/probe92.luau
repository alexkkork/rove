local mod = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
local seen, nilv, err = 0, 0, 0
local sample
local rs = game:GetService("RunService")
local conn
conn = rs.RenderStepped:Connect(function()
  for _, d in ipairs(workspace:GetDescendants()) do
    if d.Name:sub(1,10) == "$Attribute" and d:IsA("Vector3Value") then
      seen += 1
      local ok, v = pcall(function() return d.Value end)
      if not ok then err += 1; sample = sample or ("ERR "..d:GetFullName().." "..tostring(v):sub(1,60))
      elseif v == nil then nilv += 1; sample = sample or ("NIL "..d:GetFullName().." kids="..#d:GetChildren()) end
      local ok2 = pcall(function() return d.Value.X end)
      if not ok2 then err += 1; sample = sample or ("XFAIL "..d:GetFullName()) end
    end
  end
end)
task.wait(5)
conn:Disconnect()
return "seen="..seen.." nil="..nilv.." err="..err.."\n"..tostring(sample)
