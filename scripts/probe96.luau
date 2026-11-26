local m = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
local orig = m.getAttribute
local first
mod_hook = nil
m.getAttribute = function(inst, key, ...)
  local r = table.pack(pcall(orig, inst, key, ...))
  if not r[1] and not first then
    local c = inst and inst:FindFirstChild("$Attribute"..tostring(key))
    first = "inst="..tostring(inst).." key="..tostring(key).." child="..tostring(c)
      .." cls="..tostring(c and c.ClassName)
      .." val="..tostring(pcall(function() return c.Value end))
      .." props="..tostring(c and (function() local t={} for k in pairs(c:GetAttributes()) do t[#t+1]=k end return table.concat(t,",") end)())
  end
  if not r[1] then error(r[2], 0) end
  return table.unpack(r, 2, r.n)
end
task.wait(6)
return first or "no error"
