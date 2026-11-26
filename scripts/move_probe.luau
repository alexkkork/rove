local lp = game:GetService("Players").LocalPlayer
local c
for i = 1, 90 do
  c = lp.Character
  if c and c:FindFirstChild("HumanoidRootPart") then break end
  task.wait(0.5)
end
if not c then print("NOCHAR") return end
local hrp = c.HumanoidRootPart
local hum = c.Humanoid
task.wait(2)
local start = hrp.Position
hum.WalkSpeed = 24
hum:MoveTo(start + Vector3.new(0, 0, 25))
task.wait(3)
print("DIST", string.format("%.1f", (hrp.Position - start).Magnitude))
