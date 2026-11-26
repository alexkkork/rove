local out = {}
local function t(n, f)
  local r = table.pack(pcall(f))
  out[#out+1] = n.."="..tostring(r[1]).." "..(r[1] and typeof(r[2]) or tostring(r[2]):sub(1,80))
end
local model = Instance.new("Model")
local p = Instance.new("Part"); p.Size = Vector3.new(4,1,4); p.Parent = model
model.Parent = workspace
t("modelPivot", function() return model:GetPivot() end)
local terrain = workspace.Terrain
t("terrainOrigin", function() return terrain:GetOrigin() end)
t("originWs", function() return workspace:GetOrigin() end)
return table.concat(out, "\n")
