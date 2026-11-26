local m = game:GetService("ReplicatedStorage").BrickBattle.Attributes
local lp = game:GetService("Players").LocalPlayer
local bad = {}
local n = 0
local function scan(root)
  for _, d in ipairs(root:GetDescendants()) do
    local ok, attrs = pcall(function() return d:GetAttributes() end)
    if ok then
      for k, v in pairs(attrs) do
        n += 1
        local child = d:FindFirstChild("$Attribute"..k)
        if not child and #bad < 6 then bad[#bad+1] = d.Name.."."..k.."("..typeof(v)..")" end
      end
    end
  end
end
scan(lp); scan(workspace); scan(game:GetService("ReplicatedStorage"))
return "n="..n.." missing="..#bad.."\n"..table.concat(bad, "\n")
