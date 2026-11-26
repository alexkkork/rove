local mod = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
local orig = mod.getAttribute
local dump = nil
mod.getAttribute = function(inst, key, ...)
  local r = table.pack(pcall(orig, inst, key, ...))
  if not r[1] and not dump then
    local t = { "inst="..tostring(inst), "class="..tostring(inst and inst.ClassName), "key="..tostring(key) }
    for _, c in ipairs(inst:GetChildren()) do
      t[#t+1] = "child "..c.Name..":"..c.ClassName.." val="..tostring((pcall(function() return c.Value end)) and c.Value or "ERR")
    end
    local attrs = {}
    for k, v in pairs(inst:GetAttributes()) do attrs[#attrs+1] = k.."="..typeof(v)..":"..tostring(v) end
    t[#t+1] = "attrs="..table.concat(attrs, ",")
    dump = table.concat(t, "\n")
  end
  if not r[1] then error(r[2], 0) end
  return table.unpack(r, 2, r.n)
end
task.wait(6)
return dump or "no failure"
