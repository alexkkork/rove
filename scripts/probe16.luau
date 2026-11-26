local mod = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
local out = {}
local seen = 0
for _, d in ipairs(workspace:GetDescendants()) do
  local ok, attrs = pcall(function() return d:GetAttributes() end)
  if ok then
    for k in pairs(attrs) do
      seen += 1
      local r = table.pack(pcall(function() return mod.getAttribute(d, k) end))
      if not r[1] then out[#out+1] = d.Name.."."..k.." -> "..tostring(r[2]):sub(1,100) end
      if #out > 6 then break end
    end
  end
  if #out > 6 then break end
end
return "tested="..seen.."\n"..table.concat(out, "\n")
