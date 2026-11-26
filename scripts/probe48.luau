local part = Instance.new("Part")
local mesh = Instance.new("SpecialMesh")
mesh.Parent = part
local out = {}
local function t(n, f)
  local r = table.pack(pcall(f))
  out[#out+1] = n.."="..tostring(r[1]).." "..tostring(r[2]):sub(1,100)
end
t("meshId", function() return mesh.MeshId end)
t("textureId", function() return mesh.TextureId end)
t("textureID", function() return mesh.TextureID end)
t("scale", function() return mesh.Scale end)
local img = Instance.new("Decal")
t("decalTexture", function() return img.Texture end)
return table.concat(out, "\n")
