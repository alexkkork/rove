-- Test advanced humanoid controls: ChangeState, PivotTo, walkspeed changes
local lp = game:GetService("Players").LocalPlayer
local c
for i = 1, 120 do
  c = lp.Character
  if c and c:FindFirstChild("HumanoidRootPart") then break end
  task.wait(0.25)
end
if not c then print("NOCHAR") return end
print("P1 ready")
local hrp = c.HumanoidRootPart
local hum = c.Humanoid
hum.WalkSpeed = 24
-- teleport via PivotTo
local start = hrp.Position
c:PivotTo(CFrame.new(start + Vector3.new(100, 5, 100)))
task.wait(1)
print("P2 pivot done, pos:", math.floor(hrp.Position.X), math.floor(hrp.Position.Z), "dist:", math.floor((hrp.Position - (start + Vector3.new(100,5,100))).Magnitude))
-- ChangeState to Swimming then back
hum:ChangeState(Enum.HumanoidStateType.Swimming)
task.wait(0.5)
hum:ChangeState(Enum.HumanoidStateType.Running)
task.wait(0.5)
print("P3 states cycled, pos:", math.floor(hrp.Position.X), math.floor(hrp.Position.Z))
-- spin in place
for i = 1, 4 do
  hrp.CFrame = hrp.CFrame * CFrame.Angles(0, math.rad(90), 0)
  task.wait(0.3)
end
print("P4 spun 360, rotation ok")
-- walk in a square after teleport
local base = hrp.Position
hum:MoveTo(base + Vector3.new(30, 0, 0))
task.wait(2)
hum:MoveTo(base + Vector3.new(30, 0, 30))
task.wait(2)
local end_ = hrp.Position
print("P5 walk done, dist:", math.floor((end_ - base).Magnitude))
