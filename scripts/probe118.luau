local b = game:GetService("ReplicatedStorage"):FindFirstChild("_")
local out = {}
local found
for _, d in ipairs(game:GetService("Players").LocalPlayer.PlayerGui:GetDescendants()) do
  if d:IsA("InputBinding") then found = d break end
end
if not found then
  for _, d in ipairs(workspace:GetDescendants()) do if d:IsA("InputBinding") then found = d break end end
end
if not found then return "none" end
out[#out+1] = "cls="..found.ClassName
local ok, v = pcall(function() return found.KeyCode end)
out[#out+1] = "kc ok="..tostring(ok).." "..typeof(v).."="..tostring(v)
return table.concat(out, "\n")
