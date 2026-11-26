local m = game:GetService("ReplicatedFirst").ClassicLoadingScreen
local out = {}
local function t(n, f) local r=table.pack(pcall(f)) out[#out+1]=n.."="..tostring(r[1]).." "..typeof(r[2])..":"..tostring(r[2]):sub(1,110) end
t("exists", function() return m end)
t("vp", function() return workspace.CurrentCamera.ViewportSize end)
t("abs", function() return game:GetService("GuiService"):GetGuiInset() end)
p = Instance.new("Part")
t("pos0", function() return p.Position end)
t("pivot", function() return p:GetPivot() end)
t("vel", function() return p.AssemblyLinearVelocity end)
return table.concat(out, "\n")
