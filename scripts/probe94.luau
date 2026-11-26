local rs = game:GetService("RunService")
local out = {}
local function check(d)
  if d.Name:sub(1,10) ~= "$Attribute" or not d:IsA("Vector3Value") then return end
  local ok, v = pcall(function() return d.Value end)
  if not ok or v == nil then
    out[#out+1] = d:GetFullName().." ok="..tostring(ok).." v="..tostring(v).." kids="..(#d:GetChildren())
    local raw = d:GetAttributes()
    local ks = {}
    for k in pairs(raw) do ks[#ks+1]=k end
    out[#out+1] = " attrs="..table.concat(ks,",")
  end
end
for _, d in ipairs(workspace:GetDescendants()) do check(d) end
return table.concat(out, "\n")
