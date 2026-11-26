local players = game:GetService("Players")
local lp = players.LocalPlayer
local pg = lp:FindFirstChildOfClass("PlayerGui")
if not pg then pg = Instance.new("PlayerGui"); pg.Parent = lp end
print("P1 playergui:", pg ~= nil)
local sg = Instance.new("ScreenGui")
sg.Name = "RoveGUI"
sg.Parent = pg
local frame = Instance.new("Frame")
frame.Size = UDim2.fromOffset(200, 100)
frame.Position = UDim2.new(0.5, 0, 0.5, 0)
frame.BackgroundColor3 = Color3.fromRGB(30, 30, 40)
frame.Parent = sg
print("P2 frame:", frame.AbsoluteSize.X, frame.AbsoluteSize.Y)
local label = Instance.new("TextLabel")
label.Text = "rove label"
label.TextSize = 18
label.Size = UDim2.fromOffset(200, 30)
label.Parent = frame
print("P3 label text:", label.Text)
-- colors
print("P4 color:", frame.BackgroundColor3.R, frame.BackgroundColor3.G, frame.BackgroundColor3.B)
-- players
print("P5 players:", #players:GetPlayers(), "local:", lp.Name)
print("P6 char:", tostring(lp.Character ~= nil))
local hum = lp.Character and lp.Character:FindFirstChildOfClass("Humanoid")
if hum then
  print("P7 humanoid hp:", hum.Health, "state:", hum:GetState().Name)
  hum.Health = 90
  print("P8 set hp to", hum.Health)
end
frame:Destroy(); sg:Destroy()
print("P9 gui cleanup ok")
