local lp = game:GetService("Players").LocalPlayer
local c
for i = 1, 120 do
  c = lp.Character
  if c and c:FindFirstChild("HumanoidRootPart") then break end
  task.wait(0.25)
end
if not c then print("NOCHAR") return end
local hrp = c.HumanoidRootPart
local hum = c.Humanoid
-- wait until grounded (y settles) before jumping
local last = hrp.Position.Y
local stable = 0
repeat
  task.wait(0.2)
  local now = hrp.Position.Y
  if math.abs(now - last) < 0.01 then stable += 1 else stable = 0 end
  last = now
until stable >= 2
local y0 = hrp.Position.Y
print("GROUND-Y", string.format("%.2f", y0))
hum.Jump = true
local peak = 0
for i = 1, 30 do
  task.wait(0.08)
  local rise = hrp.Position.Y - y0
  if rise > peak then peak = rise end
end
print("PEAK", string.format("%.2f", peak))
