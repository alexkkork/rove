local out = {}
local function t(n, f) local r=table.pack(pcall(f)) out[#out+1]=n.."="..tostring(r[1]).." "..tostring(r[2]):sub(1,100) end
t("Map", function() return workspace:FindFirstChild("Map") end)
t("wsbox", function() return workspace:GetBoundingBox() end)
t("campivot", function() return workspace.CurrentCamera:GetPivot() end)
t("campos", function() return workspace.CurrentCamera.CFrame.Position end)
t("camcf", function() return workspace.CurrentCamera.CFrame end)
t("camfocus", function() return workspace.CurrentCamera.Focus end)
t("wsorigin", function() return workspace:GetPivot() end)
t("ws origin pos", function() return workspace:GetPivot().Position end)
t("gravity", function() return workspace.Gravity end)
t("loadingscreen model", function() return workspace:FindFirstChild("Map") and workspace.Map:GetPivot() end)
return table.concat(out, "\n")
