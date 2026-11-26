local out = {}
local function t(n, f) local r=table.pack(pcall(f)) out[#out+1]=n.."="..tostring(r[1]).." "..(r[1] and typeof(r[2])..":"..tostring(r[2]) or tostring(r[2]):sub(1,80)) end
t("get", function() return workspace:GetAttribute("CrossroadsPivot") end)
local a = workspace:GetAttributes()
local ks = {}
for k,v in pairs(a) do ks[#ks+1]=k end
out[#out+1] = "keys="..table.concat(ks, ",")
local kids = {}
for _, c in ipairs(workspace:GetChildren()) do if c.Name:sub(1,10)=="$Attribute" then kids[#kids+1]=c.Name end end
out[#out+1] = "childattrs="..table.concat(kids, ",")
return table.concat(out, "\n")
