local mod = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
local orig = mod.getAttribute
local info
mod.getAttribute = function(inst, key, ...)
  if not inst then
    if not info then
      local t = {"NIL key="..tostring(key)}
      for _, c in ipairs(workspace:GetChildren()) do
        if c:IsA("Model") and (c.Name:find("^%u")) then
          for _, d in ipairs(c:GetDescendants()) do
            if d:IsA("Tool") then t[#t+1] = "tool="..d:GetFullName().." attrs="..(#(function() local a={} for k in pairs(d:GetAttributes()) do a[#a+1]=k end return a end)()) }
          end
        end
      end
      info = table.concat(t, "\n")
    end
    return orig(inst, key, ...)
  end
  return orig(inst, key, ...)
end
task.wait(7)
return info or "no nil"
