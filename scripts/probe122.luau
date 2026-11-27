local kc = Enum.KeyCode.E
local out = {"type="..typeof(kc)}
out[#out+1] = "name="..tostring(kc.Name).." value="..tostring(kc.Value)
local ok, v = pcall(function() return kc.KeyCode end)
out[#out+1] = "kcprop ok="..tostring(ok).." "..typeof(v).."="..tostring(v)
return table.concat(out, "\n")
