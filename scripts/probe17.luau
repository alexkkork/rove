local mod = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
local out = {}
local function scan(root)
  for _, d in ipairs(root:GetDescendants()) do
    local ok, attrs = pcall(function() return d:GetAttributes() end)
    if ok then
      for k, v in pairs(attrs) do
        local r = table.pack(pcall(function() return mod.getAttribute(d, k) end))
        out[#out+1] = d.Name.."."..k.."="..typeof(v).." -> "..tostring(r[1])..":"..tostring(r[2]):sub(1,70)
      end
    end
  end
end
scan(game:GetService("Players").LocalPlayer:FindFirstChild("Backpack") or workspace)
scan(game:GetService("ReplicatedStorage"))
return table.concat(out, "\n")
