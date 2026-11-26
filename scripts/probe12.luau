local m = game:GetService("ReplicatedStorage").BrickBattle.Attributes
local ok, src = pcall(function() return decompile(m) end)
print("Y0", ok, type(src), ok and #src or tostring(src):sub(1,80))
if ok then
  local lines = string.split(src, "\n")
  local out = {}
  for i = 34, 46 do out[#out+1] = i.."="..tostring(lines[i]) end
  print(table.concat(out, "\n"))
end
