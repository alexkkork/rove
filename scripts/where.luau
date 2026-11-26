local lp = game:GetService("Players").LocalPlayer
local c
for i=1,40 do c = lp.Character; if c and c:FindFirstChild("HumanoidRootPart") then break end task.wait(.25) end
local hrp = c.HumanoidRootPart
task.wait(0.2)
local out = {"pos="..tostring(hrp.Position)}
local sizes = {}
for _, d in ipairs(workspace:GetDescendants()) do
  if d:IsA("BasePart") then
    local ok, cf = pcall(function() return d.CFrame end)
    if ok then
      local ins = (hrp.Position - cf.Position).Magnitude
      if ins < 60 then sizes[#sizes+1] = string.format("%s %s d=%.1f", d:GetFullName(), tostring(d.Size), ins) end
    end
  end
  if #sizes > 8 then break end
end
return table.concat(sizes, "\n")
