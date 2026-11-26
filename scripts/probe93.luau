local v = Instance.new("Vector3Value")
local out = {}
local function t(n, f) local r=table.pack(pcall(f)) out[#out+1]=n.."="..tostring(r[1]).." "..typeof(r[2])..":"..tostring(r[2]):sub(1,50) end
t("nan", function() v.Value = Vector3.new(0/0, 1, 1) return v.Value end)
t("nanText", function() return tostring(v.Value) end)
t("inf", function() v.Value = Vector3.new(math.huge, 0, 0) return v.Value end)
t("readback", function() return v.Value.X end)
return table.concat(out, "\n")
