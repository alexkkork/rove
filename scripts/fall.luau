local lp = game:GetService("Players").LocalPlayer
local c
for i=1,40 do c = lp.Character; if c and c:FindFirstChild("HumanoidRootPart") then break end task.wait(.25) end
if not c then return "nochar" end
local hrp = c.HumanoidRootPart
local out = {}
for i = 1, 6 do
  out[#out+1] = string.format("%.1f %.1f %.1f", hrp.Position.x, hrp.Position.y, hrp.Position.z)
  task.wait(0.5)
end
local hum = c:FindFirstChildOfClass("Humanoid")
out[#out+1] = "state="..tostring(hum and hum:GetState())
local ok,f=pcall(function() return hum.FloorMaterial end) out[#out+1]="floor="..tostring(ok)..":"..tostring(f)
return table.concat(out, "\n")
