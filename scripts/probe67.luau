-- Probe what getMapCameraCFrame could index at line 129.
local out = {}
local function t(n, f)
  local r = table.pack(pcall(f))
  out[#out+1] = n.."="..tostring(r[1]).." "..(r[1] and (typeof(r[2])..":"..tostring(r[2]):sub(1,50)) or tostring(r[2]):sub(1,70))
end
local cam = workspace.CurrentCamera
t("camCFrame.mag", function() return cam.CFrame.Position.Magnitude end)
t("camFocus.mag", function() return cam.Focus.Position.Magnitude end)
t("mapPivot.mag", function() return workspace.Map:GetPivot().Position.Magnitude end)
t("wsOrigin.mag", function() return workspace:GetOrigin().Position.Magnitude end)
t("wsOrigin", function() return workspace:GetOrigin() end)
t("mapOrigin", function() return workspace.Map:GetOrigin() end)
t("terrainPivot", function() return workspace.Terrain:GetPivot() end)
t("terrainPos.mag", function() return workspace.Terrain.Position.Magnitude end)
return table.concat(out, "\n")
