local out = {}
local function t(n, f) local r=table.pack(pcall(f)) out[#out+1]=n.."="..tostring(r[1]).." "..tostring(r[2]):sub(1,100) end
local p = Instance.new("Part")
local d = Instance.new("Decal"); d.Parent = p
local i = Instance.new("ImageLabel")
local m = Instance.new("SpecialMesh"); m.Parent = p
t("decal", function() return d.TextureId end)
t("imglbl", function() return i.TextureId end)
t("mesh", function() return m.TextureId end)
t("tooltip", function() return i.Image end)
return table.concat(out, "\n")
