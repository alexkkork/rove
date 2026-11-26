local m = game:GetService("ReplicatedFirst").ClassicLoadingScreen
local out = {}
local function t(n, f)
  local r = table.pack(pcall(f))
  local s = n.."="..tostring(r[1])
  if r[1] then s = s.." -> "..(type(r[2])=="table" and #r[2] or typeof(r[2])) else s=s.." "..tostring(r[2]):sub(1,90) end
  out[#out+1] = s
end
local kids = m:GetChildren()
out[#out+1] = "children="..#kids
for _, k in ipairs(kids) do
  t(k.Name.." pivot", function() return k:GetPivot().Position end)
  t(k.Name.." mag", function() return k:GetPivot().Position.Magnitude end)
  t(k.Name.." anchor", function() return k.AnchorPoint end)
  t(k.Name.." size", function() return k.AbsoluteSize end)
end
return table.concat(out, "\n")
