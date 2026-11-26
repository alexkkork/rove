local lp = game:GetService("Players").LocalPlayer
local c
for i = 1, 120 do
  c = lp.Character
  if c and c:FindFirstChild("HumanoidRootPart") then break end
  task.wait(0.25)
end
if not c then print("NOCHAR") return end
print("P1 char ready")
local hrp = c.HumanoidRootPart
local hum = c.Humanoid
hum.WalkSpeed = 24
local start = hrp.Position
print("P2 start", math.floor(start.X), math.floor(start.Z))
hum:MoveTo(start + Vector3.new(60, 0, 0))
task.wait(3)
print("P3 leg1", math.floor((hrp.Position - (start + Vector3.new(60,0,0))).Magnitude))
hum:MoveTo(start + Vector3.new(60, 0, 60))
task.wait(3)
hum:MoveTo(start + Vector3.new(0, 0, 60))
task.wait(3)
local now = hrp.Position
print("P4 dist", math.floor((now - start).Magnitude))
