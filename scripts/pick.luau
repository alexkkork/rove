local vu = game:GetService("VirtualUser")
local lp = game:GetService("Players").LocalPlayer
local cam = workspace.CurrentCamera
local hrp = lp.Character:WaitForChild("HumanoidRootPart")
print("K1 hrp", hrp.Position)
cam.CameraType = Enum.CameraType.Scriptable
cam.CFrame = CFrame.new(hrp.Position + Vector3.new(0, 8, 14), hrp.Position)
print("K2 cam", cam.CFrame.Position == cam.CFrame.Position)
task.wait(0.3)
vu:CaptureController()
vu:MoveMouse(Vector2.new(960, 540))
task.wait(0.4)
local m = lp:GetMouse()
print("K3 target", tostring(m.Target), "dist", tostring(m.Hit.Position))
print("K4 surf", tostring(m.TargetSurface), "views", m.ViewSizeX, m.ViewSizeY)
print("K5 unitray", typeof(m.UnitRay), tostring(m.UnitRay.Origin))
-- raycast the same screen point directly for cross-check
local ray = cam:ScreenPointToRay(960, 540)
local hit = workspace:Raycast(ray.Origin, ray.Direction * 200)
print("K6 raycast", hit and hit.Instance.Name, hit and math.floor(hit.Distance))
vu:ClickButton1(Vector2.new(960, 540))
task.wait(0.2)
print("K7 clicked ok")
