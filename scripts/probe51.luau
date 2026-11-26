local m = game:GetService("ReplicatedFirst").ClassicLoadingScreen
local orig = m.Source
local box = Instance.new("Folder"); box.Name="__lbg"; box.Parent = workspace
local out = {}
local function t(n, f) local r=table.pack(pcall(f)) out[#out+1]=n.."="..tostring(r[1]).." "..tostring(r[2]):sub(1,140) end
for _, n in ipairs({"GetChildren","GetDescendants","WaitForChild","FindFirstChild","FindFirstChildWhichIsA","GetAttribute","GetAttributes","GetTags","GetPivot","GetBoundingBox","GetExtentsSize","GetPositionOnScreen"}) do
  local kids = m:GetChildren()
  if #kids == 0 then out[#out+1] = "no children"; break end
  t(n, function() return m[kids[1].Name][n] and m[kids[1].Name]:GetChildren() end)
end
return table.concat(out, "\n")
