local p = Instance.new("Part")
p:SetAttribute("A", Vector2.new(3,4))
p:SetAttribute("B", Vector3.new(1,2,3))
p:SetAttribute("C", "str")
p:SetAttribute("D", true)
p:SetAttribute("E", CFrame.new(1,2,3))
p:SetAttribute("F", Color3.new(1,0,0))
p:SetAttribute("G", 9)
local out = {}
for _, k in ipairs({"A","B","C","D","E","F","G","Z"}) do
  local v = p:GetAttribute(k)
  out[#out+1] = k.."="..typeof(v)..":"..tostring(v)
end
return table.concat(out, "\n")
