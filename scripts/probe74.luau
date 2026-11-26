local out = {}
local ok, attrs = pcall(function() return workspace:GetAttributes() end)
out[#out+1] = "ok="..tostring(ok).." "..tostring(attrs)
if ok then for k,v in pairs(attrs) do out[#out+1]=k.."="..typeof(v)..":"..tostring(v) end end
local c = workspace:FindFirstChild("$AttributeCrossroadsPivot")
out[#out+1] = "child="..tostring(c).." val="..tostring(c and c.Value)
return table.concat(out, "\n")
