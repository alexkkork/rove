local m = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
local orig = m.getAttribute
local first
m.getAttribute = function(inst, key, ...)
  local r = table.pack(pcall(orig, inst, key, ...))
  if not r[1] and not first then
    local c = inst and inst:FindFirstChild("$Attribute"..tostring(key))
    local rv = c and table.pack(pcall(function() return c.Value end))
    first = "inst="..tostring(inst).." key="..tostring(key).." child="..tostring(c).." cls="..tostring(c and c.ClassName)
      .." read="..tostring(rv and rv[1]).." "..tostring(rv and rv[2])
  end
  if not r[1] then error(r[2], 0) end
  return table.unpack(r, 2, r.n)
end
task.wait(7)
return first or "none"
