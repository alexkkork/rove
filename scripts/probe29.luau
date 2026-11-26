local mod = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
local orig = mod.getAttribute
local log = {}
mod.getAttribute = function(inst, key, ...)
  if not inst then
    if #log < 8 then log[#log+1] = "NIL INST key="..tostring(key) end
    return orig(inst, key, ...)
  end
  local r = table.pack(pcall(orig, inst, key, ...))
  if not r[1] and #log < 8 then
    log[#log+1] = tostring(inst):sub(1,40).." key="..tostring(key).." err="..tostring(r[2]):sub(1,120)
  end
  if not r[1] then error(r[2], 0) end
  return table.unpack(r, 2, r.n)
end
task.wait(6)
local container = workspace:FindFirstChild("__probelog") or Instance.new("Folder")
container.Name="__probelog"; container.Parent=workspace
local s = Instance.new("StringValue"); s.Name="LOG"; s.Value = table.concat(log, "\n"); s.Parent=container
return "n="..#log.."\n"..table.concat(log,"\n")
