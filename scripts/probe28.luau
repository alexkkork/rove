local mod = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
local lp = game:GetService("Players").LocalPlayer
task.wait(2)
local out = {}
local n = 0
local function scan(root)
  for _, d in ipairs(root:GetDescendants()) do
    local ok, attrs = pcall(function() return d:GetAttributes() end)
    if ok then
      for k in pairs(attrs) do
        n += 1
        local r = table.pack(pcall(function() return mod.getAttribute(d, k) end))
        if not r[1] or r[2] == nil then
          out[#out+1] = d:GetFullName().."."..k.." -> "..tostring(r[1]).."/"..tostring(r[2]):sub(1,80)
        end
      end
    end
  end
end
scan(lp)
scan(workspace)
return "n="..n.." bad="..#out.."\n"..table.concat(out, "\n")
