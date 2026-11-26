local m = game:GetService("ReplicatedStorage").BrickBattle.Attributes
local ok, mod = pcall(require, m)
print("W0 require", ok, tostring(mod))
local src = m.Source
print("W1 source", type(src), src and #src or -1)
