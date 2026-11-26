local ls = game:GetService("LocalizationService")
local out = {"cls="..ls.ClassName}
out[#out+1] = "fn="..tostring(ls.GetTranslatorForPlayer)
local ok, tr = pcall(function() return ls:GetTranslatorForPlayer(game:GetService("Players").LocalPlayer) end)
out[#out+1] = "call="..tostring(ok).." "..tostring(tr)
if ok and tr then
  out[#out+1] = "trcls="..tr.ClassName
  out[#out+1] = "tr="..tostring(tr:Translate("hello"))
end
return table.concat(out, "\n")
