local out = {}
local function t(n, f) local r=table.pack(pcall(f)) out[#out+1]=n.."="..tostring(r[1]).." "..(r[1] and typeof(r[2])..":"..tostring(r[2]) or tostring(r[2]):sub(1,70)) end
t("set", function() workspace:SetAttribute("Zed", 5) return "ok" end)
t("get", function() return workspace:GetAttribute("Zed") end)
t("child", function() return workspace:FindFirstChild("$AttributeZed") end)
t("childval", function() return workspace:FindFirstChild("$AttributeZed").Value end)
local m = workspace:FindFirstChild("Crossroads")
t("modelSet", function() m:SetAttribute("Q", 3) return "ok" end)
t("modelGet", function() return m:GetAttribute("Q") end)
return table.concat(out, "\n")
