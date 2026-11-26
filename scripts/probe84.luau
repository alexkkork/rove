local mod = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
-- call getAttribute on every (inst,key) pair the failing game frames use
local out = {}
local function t(inst, key)
  local r = table.pack(pcall(mod.getAttribute, inst, key))
  if not r[1] then out[#out+1] = tostring(inst and inst:GetFullName()).."."..tostring(key).." -> "..tostring(r[2]):sub(1,90) end
end
for _, d in ipairs(workspace:GetDescendants()) do
  local ok, attrs = pcall(function() return d:GetAttributes() end)
  if ok then for k in pairs(attrs) do t(d, k) end end
end
return "n="..#out.."\n"..table.concat(out, "\n")
