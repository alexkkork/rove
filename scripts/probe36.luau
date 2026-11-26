local mod = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
task.wait(6)
local sword = game:GetService("Players").LocalPlayer.Character:FindFirstChildWhichIsA("Tool")
if not sword then
  local bp = game:GetService("Players").LocalPlayer:FindFirstChild("Backpack")
  sword = bp and bp:FindFirstChildWhichIsA("Tool")
end
local out = {"tool="..tostring(sword)}
local c = sword:FindFirstChild("$AttributeIsLunging")
if c then
  local r = table.pack(pcall(function() return c.Value end))
  out[#out+1] = "class="..c.ClassName.." read="..tostring(r[1]).." "..tostring(r[2]):sub(1,160)
  local r2 = table.pack(pcall(function() c.Value = Vector3.new(1,2,3) end))
  out[#out+1] = "write="..tostring(r2[1]).." "..tostring(r2[2]):sub(1,160)
else
  out[#out+1] = "no child; kids="..table.concat((function() local t={} for _,k in ipairs(sword:GetChildren()) do t[#t+1]=k.Name..":"..k.ClassName end return t end)(), ",")
end
return table.concat(out, "\n")
