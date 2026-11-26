local c = workspace:FindFirstChild("Crossroads")
local out = {}
local function t(n, f) local r=table.pack(pcall(f)) out[#out+1]=n.."="..tostring(r[1]).." "..(r[1] and typeof(r[2]) or tostring(r[2]):sub(1,70)) end
t("pivotAttr", function() return workspace:GetAttribute("CrossroadsPivot") end)
t("extentsAttr", function() return workspace:GetAttribute("CrossroadsExtentsSize") end)
t("pivotMag", function() return workspace:GetAttribute("CrossroadsPivot").Position.Magnitude end)
t("model", function() return c end)
t("modelOrigin", function() return c:GetOrigin() end)
t("modelOriginMag", function() return c:GetOrigin().Position.Magnitude end)
t("box", function() return c:GetBoundingBox() end)
return table.concat(out, "\n")
