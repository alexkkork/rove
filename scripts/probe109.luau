local pg = game:GetService("Players").LocalPlayer:FindFirstChild("PlayerGui")
local sg = pg:FindFirstChildWhichIsA("ScreenGui")
local out = {}
local function t(n, f) local r=table.pack(pcall(f)) out[#out+1]=n.."="..tostring(r[1]).." "..typeof(r[2])..":"..tostring(r[2]):sub(1,70) end
t("pg", function() return pg end)
t("sg", function() return sg end)
if sg then
  local root = sg:FindFirstChild("Root") or sg:GetChildren()[1]
  t("root", function() return root end)
  if root then
    t("abs", function() return root.AbsoluteSize end)
    t("absy", function() return root.AbsoluteSize.Y end)
  end
end
return table.concat(out, "\n")
