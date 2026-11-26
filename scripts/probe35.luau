local out = {}
local function t(n, f)
  local r = table.pack(pcall(f))
  out[#out+1] = n.."="..tostring(r[1]).." "..tostring(r[2]):sub(1,120)
end
local v = Instance.new("Vector3Value")
t("new", function() return v end)
t("set", function() v.Value = Vector3.new(1,2,3) return v.Value end)
local c = Instance.new("Vector3Value"); c.Value = Vector3.new(4,5,6)
t("preset", function() return c.Value end)
local sv = Instance.new("StringValue"); sv.Value = "x"
t("str", function() return sv.Value end)
local bv = Instance.new("BoolValue"); bv.Value = true
t("bool", function() return bv.Value end)
return table.concat(out, "\n")
