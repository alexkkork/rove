local pg = game:GetService("Players").LocalPlayer:FindFirstChild("PlayerGui")
local sg = Instance.new("ScreenGui")
local outer = Instance.new("Frame"); outer.Size = UDim2.fromOffset(300,200); outer.Parent = sg
local inner = Instance.new("Frame"); inner.Size = UDim2.fromOffset(100,50); inner.Parent = outer
sg.Parent = pg
local out = {}
local function t(n, f) local r=table.pack(pcall(f)) out[#out+1]=n.."="..tostring(r[1]).." "..typeof(r[2])..":"..tostring(r[2]):sub(1,40) end
t("outer", function() return outer.AbsoluteSize end)
t("inner", function() return inner.AbsoluteSize end)
t("innerY", function() return inner.AbsoluteSize.Y end)
t("arith", function() return inner.AbsoluteSize.Y - 16 end)
return table.concat(out, "\n")
