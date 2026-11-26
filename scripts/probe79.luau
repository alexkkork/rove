local mod = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
local orig = mod.getAttribute
local info
mod.getAttribute = function(inst, key, ...)
  local r = table.pack(pcall(orig, inst, key, ...))
  if not r[1] and not info then info = "inst="..tostring(inst).." key="..tostring(key) end
  if not r[1] then error(r[2], 0) end
  return table.unpack(r, 2, r.n)
end
task.wait(6)
return info or "no error"
