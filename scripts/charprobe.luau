local lp = game:GetService("Players").LocalPlayer
print("P", lp.Name, "char:", tostring(lp.Character))
task.wait(8)
local c = lp.Character
print("P2 char:", tostring(c))
local ws = game:GetService("Workspace")
local n, mine = 0, 0
for _, ch in ipairs(ws:GetChildren()) do
  if ch:IsA("Model") then
    n += 1
    if ch.Name == lp.Name then mine += 1 end
  end
end
print("models:", n, "matching me:", mine)
