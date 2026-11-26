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
-- walk away then jump
hum.WalkSpeed = 24
local start = hrp.Position
hum:MoveTo(start + Vector3.new(40, 0, 0))
task.wait(2.5)
print("P2 walked", math.floor((hrp.Position - start).Magnitude))
-- jump mid-air check
local y0 = hrp.Position.Y
hum.Jump = true
local peak = 0
for i = 1, 30 do
  task.wait(0.08)
  local rise = hrp.Position.Y - y0
  if rise > peak then peak = rise end
end
print("P3 jump peak", string.format("%.1f", peak))
-- jump again after landing
task.wait(1.5)
local y1 = hrp.Position.Y
hum.Jump = true
for i = 1, 30 do
  task.wait(0.08)
  local rise = hrp.Position.Y - y1
  if rise > peak then peak = rise end
end
print("P4 second jump", string.format("%.1f", peak))
