local lp = game:GetService("Players").LocalPlayer
local c
for i=1,40 do c = lp.Character; if c and c:FindFirstChild("HumanoidRootPart") then break end task.wait(.25) end
if not c then return "nochar" end
local hrp = c.HumanoidRootPart
task.wait(1.5)
local out = {"y="..string.format("%.1f", hrp.Position.y)}
local r = workspace:Raycast(hrp.Position, Vector3.new(0,-400,0))
out[#out+1] = "downhit="..tostring(r and r.Instance and r.Instance:GetFullName())
local up = workspace:Raycast(hrp.Position, Vector3.new(0,400,0))
out[#out+1] = "uphit="..tostring(up and up.Instance and up.Instance:GetFullName())
out[#out+1] = "grounded="..tostring(hrp.Position.y)
out[#out+1] = "terrainsize="..tostring(workspace.Terrain.Size)
return table.concat(out, "\n")
