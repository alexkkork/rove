local mod = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
local orig = mod.getAttribute
local first
mod.getAttribute = function(inst, key, ...)
  local r = table.pack(pcall(orig, inst, key, ...))
  if not r[1] and not first then
    first = "inst="..tostring(inst).." cls="..tostring(inst.ClassName).." key="..tostring(key)
      .." kids="..(function() local t={} for _,c in ipairs(inst:GetChildren()) do t[#t+1]=c.Name..":"..c.ClassName end return table.concat(t,",") end)()
      .." attr="..tostring(inst:GetAttribute(key))
  end
  if not r[1] then error(r[2], 0) end
  return table.unpack(r, 2, r.n)
end
task.wait(6)
return first or "no error"
