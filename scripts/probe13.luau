local chars = game:GetService("Players"):GetPlayers()
local out = {}
for _, p in ipairs(chars) do
  local c = p.Character
  if c then
    for _, d in ipairs(c:GetDescendants()) do
      local ok, attrs = pcall(function() return d:GetAttributes() end)
      if ok then
        for k, v in pairs(attrs) do
          out[#out+1] = d.Name.."."..k.."="..typeof(v)..":"..tostring(v)
        end
      end
    end
  end
end
local m = game:GetService("ReplicatedStorage").BrickBattle.Attributes
local ok, mod = pcall(require, m)
out[#out+1] = "require="..tostring(ok).." "..tostring(mod)
return table.concat(out, "\n")
