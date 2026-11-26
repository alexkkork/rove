local out = {}
local function t(n, f)
  local r = table.pack(pcall(f))
  local s = n.."="..tostring(r[1]).." n="..(r.n-1)
  for i=2,r.n do s = s.." | "..typeof(r[i])..":"..tostring(r[i]):sub(1,50) end
  out[#out+1] = s
end
local map = workspace:FindFirstChild("Map") or workspace
t("bbox", function() return workspace:GetBoundingBox() end)
t("mapbox", function() return map:GetBoundingBox() end)
t("mappivot", function() return map:GetPivot() end)
t("camposmag", function() return workspace.CurrentCamera.CFrame.Position.Magnitude end)
t("mapparts", function() return #map:GetChildren() end)
local ok, cf, sz = pcall(function() return map:GetBoundingBox() end)
if ok then
  out[#out+1] = "pos.mag="..tostring(cf.Position.Magnitude).." size.mag="..tostring(sz.Magnitude)
end
return table.concat(out, "\n")
