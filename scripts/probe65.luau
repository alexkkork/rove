local out = {}
local function t(n, f) local r=table.pack(pcall(f)) out[#out+1]=n.."="..tostring(r[1]).." "..tostring(r[2]):sub(1,150) end
t("tbox", function() return workspace.Terrain:GetBoundingBox() end)
t("tExtents", function() return workspace.Terrain:GetExtentsSize() end)
t("tChildCount", function() return #workspace.Terrain:GetChildren() end)
t("camChar", function() return game:GetService("Players").LocalPlayer.Character end)
t("camSubject", function() return workspace.CurrentCamera.CameraSubject end)
return table.concat(out, "\n")
