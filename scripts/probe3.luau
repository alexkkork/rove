local rs = game:GetService("ReplicatedStorage")
local function find(name, root)
  for _, d in ipairs(root:GetDescendants()) do
    if d.Name == name then return d end
  end
end
local bb = find("BrickBattle", rs)
print("P0", tostring(bb))
local mod = bb and find("WatchMouseCursor", bb)
print("P1", tostring(mod))
if mod and mod:IsA("ModuleScript") then
  local ok, res = pcall(require, mod)
  print("P2 require", ok, tostring(res))
end
