local ws = game:GetService("Workspace")
local rs = game:GetService("ReplicatedStorage")
-- attributes
local p = Instance.new("Part")
p.Name = "RoveTestPart"
p.Size = Vector3.new(4, 1, 4)
p.Anchored = true
p.Parent = ws
print("P1 instance created at", p.Position)
p:SetAttribute("RoveKey", 42)
print("P2 attr:", tostring(p:GetAttribute("RoveKey")))
p:AddTag("RoveTag")
print("P3 tag:", tostring(p:HasTag("RoveTag")))
-- move it
p.Position = Vector3.new(10, 20, 10)
print("P4 moved to", p.Position)
-- tween
local TweenService = game:GetService("TweenService")
local t = TweenService:Create(p, TweenInfo.new(0.4), { Transparency = 1 })
t:Play()
task.wait(0.7)
print("P5 tween done, transparency", string.format("%.2f", p.Transparency))
-- find by name
local found = ws:FindFirstChild("RoveTestPart")
print("P6 found:", found ~= nil, "class:", found and found.ClassName or "nil")
-- collection service
local cs = game:GetService("CollectionService")
print("P7 tagged count:", #cs:GetTagged("RoveTag"))
-- destroy
p:Destroy()
task.wait(0.2)
print("P8 destroyed, child gone:", ws:FindFirstChild("RoveTestPart") == nil)
