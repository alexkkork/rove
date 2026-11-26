local cam = workspace.CurrentCamera
local out = {}
local function t(name, f)
  local ok, v = pcall(f)
  table.insert(out, name .. "=" .. tostring(ok) .. ":" .. tostring(v and tostring(v):sub(1, 60)))
end
t("cframe", function() return cam.CFrame.Position end)
t("focus", function() return cam.Focus.Position end)
t("vpX", function() return cam.ViewportSize.X end)
t("vpx", function() return cam.ViewportSize.x end)
t("pX", function() return cam.CFrame.p.X end)
t("terrainPos", function() return workspace.Terrain.Position end)
t("terrainSize", function() return workspace.Terrain.Size end)
return table.concat(out, " | ")
