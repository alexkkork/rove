local lp = game:GetService("Players").LocalPlayer
task.wait(10)
local c = lp.Character
if not c then print("NOCHAR") return end
print("char:", c.Name, "parent:", tostring(c.Parent and c.Parent.ClassName))
print("PrimaryPart:", tostring(c.PrimaryPart))
local names = {}
for i, ch in ipairs(c:GetChildren()) do
  if i <= 15 then names[#names+1] = ch.ClassName .. ":" .. ch.Name end
end
print("children:", #c:GetChildren(), table.concat(names, " | "))
local hum = c:FindFirstChildOfClass("Humanoid")
print("humanoid:", tostring(hum), "health:", hum and hum.Health or -1)
