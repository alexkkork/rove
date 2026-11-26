local svc = game:GetService("LocalizationService")
local meta = getmetatable(svc)
local out = {"mt="..tostring(meta)}
for k, v in pairs(meta) do out[#out+1] = tostring(k).."="..typeof(v) end
local idx = meta.__index
if type(idx) == "function" then
  local ok, v = pcall(idx, svc, "GetCorescriptLocalizations")
  out[#out+1] = "idxfn="..tostring(ok).." "..typeof(v)
end
return table.concat(out, "\n")
