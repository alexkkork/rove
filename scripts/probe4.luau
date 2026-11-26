local rs = game:GetService("ReplicatedStorage")
local bb = rs.BrickBattle
local mod = bb.WatchMouseCursor
print("Q0", mod.ClassName, mod:IsA("ModuleScript"))
local ok, res = pcall(function() return require(mod) end)
print("Q1 require", ok, tostring(res))
local ok2, err = pcall(debug.info, mod, 1)
print("Q2 info", ok2, tostring(err))
print("Q3 src", #(mod.Source or ""))
