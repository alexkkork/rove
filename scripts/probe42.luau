local bb = game:GetService("ReplicatedStorage").BrickBattle
local sword = bb.Sword
local out = {}
-- brute check names used around grip processing
for _, n in ipairs({"processGrip","Grip","processSwing","getAttribute","lerp","CFrame.lookAt","tick"}) do
  out[#out+1] = n.."="..tostring(rawget(sword, n))
end
local ok, mod = pcall(require, sword)
out[#out+1] = "require="..tostring(ok).." "..typeof(mod)
if ok and typeof(mod)=="table" then
  for k, v in pairs(mod) do out[#out+1] = "mod."..tostring(k).."="..typeof(v) end
end
return table.concat(out, "\n")
