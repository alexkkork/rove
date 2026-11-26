local m = game:GetService("ReplicatedStorage").BrickBattle.Attributes
local src = m.Source
print("T0 len", src and #src)
if src then
  local lines = string.split(src, "\n")
  for i = 30, 50 do print("T"..i, lines[i]) end
end
local s = game:GetService("ReplicatedStorage").BrickBattle.Sword
local src2 = s.Source
if src2 then
  local l2 = string.split(src2, "\n")
  for i = 360, 372 do print("U"..i, l2[i]) end
end
