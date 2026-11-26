local out = {}
local function t(n, f) local r=table.pack(pcall(f)) out[#out+1]=n.."="..tostring(r[1]).." "..(r[1] and typeof(r[2]) or tostring(r[2]):sub(1,70)) end
local cam = workspace.CurrentCamera
t("subj", function() return cam.CameraSubject end)
t("subjpos", function() return cam.CameraSubject.Position end)
t("subjmag", function() return cam.CameraSubject.Position.Magnitude end)
t("focusmag", function() return cam.Focus.Position.Magnitude end)
t("wsAttrPivot", function() return workspace:GetAttribute("CrossroadsPivot") end)
t("wsAttrExt", function() return workspace:GetAttribute("CrossroadsExtentsSize") end)
t("origin", function() return workspace:GetOrigin().Position end)
t("originmag", function() return workspace:GetOrigin().Position.Magnitude end)
t("char", function() return game:GetService("Players").LocalPlayer.Character end)
t("pivotOffset", function() return workspace:GetPivot().Position end)
return table.concat(out, "\n")
