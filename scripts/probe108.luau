local out = {}
local function t(n, f) local r=table.pack(pcall(f)) out[#out+1]=n.."="..tostring(r[1]).." "..typeof(r[2])..":"..tostring(r[2]):sub(1,80) end
local sg = Instance.new("ScreenGui")
local f = Instance.new("Frame"); f.Size = UDim2.fromOffset(200,100); f.Parent = sg
sg.Parent = game:GetService("Players").LocalPlayer:FindFirstChild("PlayerGui")
t("abs", function() return f.AbsoluteSize end)
t("absY", function() return f.AbsoluteSize.Y end)
t("abspos", function() return f.AbsolutePosition end)
t("rootAbs", function() return f.AbsoluteSize.Y - 16 end)
return table.concat(out, "\n")
