local out = {}
local function t(n, f) local r=table.pack(pcall(f)) out[#out+1]=n.."="..tostring(r[1]).." "..(r[1] and typeof(r[2]) or tostring(r[2]):sub(1,80)) end
t("batch", function() return workspace:GetAttribute("Batch") end)
t("pivot", function() return workspace:GetAttribute("CrossroadsPivot") end)
t("extents", function() return workspace:GetAttribute("CrossroadsExtentsSize") end)
t("ssa", function() return workspace.StreamingEnabled end)
t("wsOrigin", function() return workspace:GetOrigin() end)
t("wsExtents", function() return workspace:GetExtentsSize() end)
t("wsBox", function() return workspace:GetBoundingBox() end)
t("wsPivot", function() return workspace:GetPivot() end)
return table.concat(out, "\n")
