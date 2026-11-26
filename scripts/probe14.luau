local out = {}
local function t(n, f)
  local r = table.pack(pcall(f))
  local parts = {n.."="..tostring(r[1])}
  for i=2,r.n do parts[#parts+1]=typeof(r[i])..":"..tostring(r[i]) end
  out[#out+1]=table.concat(parts, " ")
end
t("inset", function() return game:GetService("GuiService"):GetGuiInset() end)
t("abspos", function() local g=Instance.new("ScreenGui"); local f=Instance.new("Frame"); f.Parent=g; return f.AbsolutePosition end)
t("viewport", function() return workspace.CurrentCamera.ViewportSize end)
t("mouseloc", function() return game:GetService("UserInputService"):GetMouseLocation() end)
t("guiobjs", function() return game:GetService("Players").LocalPlayer:FindFirstChild("PlayerGui"):GetGuiObjectsAtPosition(10,10) end)
return table.concat(out, "\n")
