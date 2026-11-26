local out = {}
local function scan(root, label)
  for _, d in ipairs(root:GetDescendants()) do
    if d:IsA("Animation") then
      out[#out+1] = label.."/"..d.Name.." id="..tostring(d.AnimationId)
    end
  end
end
scan(workspace, "ws")
scan(game:GetService("Players").LocalPlayer, "plr")
for _, p in ipairs(game:GetService("Players"):GetPlayers()) do
  if p.Character then scan(p.Character, p.Name) end
end
return "n="..#out.."\n"..table.concat(out, "\n")
