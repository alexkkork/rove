local lp = game:GetService("Players").LocalPlayer
local part = Instance.new("Part"); part.Parent = workspace
local out = {}
local function t(n, f)
  local r = table.pack(pcall(f))
  out[#out+1] = n.."="..tostring(r[1]).." "..typeof(r[2])..":"..tostring(r[2]):sub(1, 60)
end
t("players", function() return game:GetService("Players"):GetPlayers() end)
t("tagged", function() return game:GetService("CollectionService"):GetTagged("x") end)
t("alltags", function() return game:GetService("CollectionService"):GetAllTags() end)
t("pg", function() return lp:FindFirstChild("PlayerGui"):GetChildren() end)
t("box", function() return workspace:GetPartBoundsInBox(CFrame.new(0,0,0), Vector3.new(100,100,100)) end)
t("radius", function() return workspace:GetPartBoundsInRadius(Vector3.new(0,0,0), 100) end)
t("conn", function() return part:GetConnectedParts() end)
t("joints", function() return part:GetJoints() end)
t("touch", function() return part:GetTouchingParts() end)
t("attr", function() return part:GetAttributes() end)
t("children", function() return workspace:GetChildren() end)
t("desc", function() return workspace:GetDescendants() end)
t("tags", function() return part:GetTags() end)
t("guis", function() return lp:FindFirstChild("PlayerGui"):GetGuiObjectsAtPosition(1,1) end)
t("teams", function() return game:GetService("Teams"):GetTeams() end)
t("bindings", function() return game:GetService("ContextActionService"):GetAllBoundActionInfo() end)
return table.concat(out, "\n")
