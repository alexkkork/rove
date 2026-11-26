local out = {}
local function t(n, f)
  local r = table.pack(pcall(f))
  local s = n.."="..tostring(r[1]).." n="..(r.n-1)
  for i=2,r.n do
    s = s.." | "..typeof(r[i])
    local ok2, m = pcall(function() return r[i].Magnitude end)
    s = s.."(mag="..tostring(ok2)..")"
  end
  out[#out+1] = s
end
local map = workspace:FindFirstChild("Map") or workspace
t("terrainBox", function() return workspace.Terrain:GetBoundingBox() end)
t("terrainExt", function() return workspace.Terrain:GetExtentsSize() end)
t("mapExt", function() return map:GetExtentsSize() end)
t("mapScale", function() return map:GetScale() end)
t("wsExt", function() return workspace:GetExtentsSize() end)
t("camVP", function() return workspace.CurrentCamera.ViewportSize end)
t("camFOV", function() return workspace.CurrentCamera.FieldOfView end)
t("camSubj", function() return workspace.CurrentCamera.CameraSubject end)
return table.concat(out, "\n")
