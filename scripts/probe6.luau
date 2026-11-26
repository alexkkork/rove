-- Find every BasePart in workspace and report which lack CFrame/Position
local bad, good = 0, 0
local sample
for _, d in ipairs(workspace:GetDescendants()) do
  if d:IsA("BasePart") then
    local ok, pos = pcall(function() return d.Position end)
    if ok then good += 1 else bad += 1; sample = sample or (d:GetFullName() .. " -> " .. tostring(pos)) end
  end
end
print("S0 good", good, "bad", bad, tostring(sample))
-- Also check Camera.CFrame reads and CFrame.new(p) path
local m = game:GetService("Players").LocalPlayer:GetMouse()
print("S1 hit", tostring(m.Hit.Position))
print("S2 origin", tostring(m.Origin.Position))
