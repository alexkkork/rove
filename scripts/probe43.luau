local sword = game:GetService("ReplicatedStorage").BrickBattle.Sword
local out = {"type="..typeof(sword)}
local ok, mod = pcall(require, sword)
out[#out+1] = "require="..tostring(ok).." "..typeof(mod)
if ok and typeof(mod)=="table" then
  local ks = {}
  for k, v in pairs(mod) do ks[#ks+1] = tostring(k).."="..typeof(v) end
  out[#out+1] = "keys="..table.concat(ks, ",")
end
return table.concat(out, "\n")
