-- Find a ProximityPrompt in the world, aim the camera at it, click it.
local vu = game:GetService("VirtualUser")
local lp = game:GetService("Players").LocalPlayer
local char = lp.Character or lp.CharacterAdded:Wait()
local hrp = char:WaitForChild("HumanoidRootPart")
local cam = workspace.CurrentCamera
cam.CameraType = Enum.CameraType.Scriptable

local prompt, part
for _, d in ipairs(workspace:GetDescendants()) do
  if d:IsA("ProximityPrompt") and d.Enabled and d.Parent and d.Parent:IsA("BasePart") then
    prompt, part = d, d.Parent
    break
  end
end
print("C1 prompt", tostring(prompt), tostring(part))
if not prompt then print("C2 none"); return end


-- Move next to the prompt
local target = part.Position + Vector3.new(0, 2, 4)
hrp.CFrame = CFrame.new(target)
task.wait(0.3)
cam.CFrame = CFrame.new(target + Vector3.new(0, 3, 8), part.Position)
task.wait(0.3)

local fired = 0
prompt.Triggered:Connect(function() fired += 1; print("C3 TRIGGERED", d and "" or "") end)
vu:CaptureController()
vu:MoveMouse(Vector2.new(960, 540))
task.wait(0.4)
local m = lp:GetMouse()
print("C4 mouse target", tostring(m.Target), "dist", tostring(m.Hit.Position))
local ok, err = pcall(function() vu:ClickButton1(Vector2.new(960, 540)) end)
print("C5 click", ok, tostring(err))
task.wait(0.5)
print("C6 fired", fired)
