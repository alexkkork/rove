local lp = game:GetService("Players").LocalPlayer
task.wait(2)
local pg = lp:WaitForChild("PlayerGui", 10)
print("R0 pg", tostring(pg))
local ok, res = pcall(function() return pg:GetGuiObjectsAtPosition(960, 540) end)
print("R1 guiobjs", ok, typeof(res), res and #res)
local m = lp:GetMouse()
print("R2 target", tostring(m.Target), "uis", tostring(game:GetService("UserInputService"):GetMouseLocation()))
