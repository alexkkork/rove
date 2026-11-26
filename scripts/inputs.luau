-- Exercise input APIs: VirtualUser, Mouse, camera, keypress simulation
local vu = game:GetService("VirtualUser")
local players = game:GetService("Players")
local lp = players.LocalPlayer
print("P1 VirtualUser:", vu ~= nil)
local uc = game:GetService("UserInputService")
print("P2 UIS:", uc ~= nil, "mouse enabled:", tostring(uc.MouseEnabled))
local mouse = lp:GetMouse()
print("P3 mouse:", mouse ~= nil, "hit:", tostring(mouse.Hit))
print("P4 mouse pos:", tostring(mouse.X), tostring(mouse.Y))
-- fire a key event locally
pcall(function()
  local kc = Enum.KeyCode.Space
  print("P5 keycode:", kc.Name, kc.Value)
end)
-- camera
local cam = workspace.CurrentCamera
print("P6 camera:", cam ~= nil, "type:", tostring(cam and cam.CameraType))
if cam then
  cam.CFrame = CFrame.new(Vector3.new(0, 50, 0))
  print("P7 camera moved, y:", math.floor(cam.CFrame.Position.Y))
end
-- VU button
local ok, err = pcall(function() vu:Button1Down(Vector2.new(100, 100)) end)
print("P8 Button1Down:", ok and "ok" or tostring(err):sub(1, 50))
-- jumped event
local ok2 = pcall(function() vu:CaptureController(); vu:ClickButton1(Vector2.new(50, 50)) end)
print("P9 CaptureController+Click:", ok2 and "ok" or "fail")
