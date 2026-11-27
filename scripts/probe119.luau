local b
for _, d in ipairs(workspace:GetDescendants()) do if d:IsA("InputBinding") then b = d break end end
if not b then
  b = Instance.new("InputBinding")
  b.KeyCode = Enum.KeyCode.E
end
local out = {"cls="..b.ClassName}
local ok, v = pcall(function() return b.KeyCode end)
out[#out+1] = "ok="..tostring(ok).." "..typeof(v).."="..tostring(v)
if ok and typeof(v) == "EnumItem" then out[#out+1] = "name="..v.Name.." value="..v.Value end
return table.concat(out, "\n")
