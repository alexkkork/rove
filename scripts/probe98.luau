local ls = game:GetService("LocalizationService")
local info = game:GetService("RunService")
local out = {"ls="..ls.ClassName}
out[#out+1] = "lsfns=" .. tostring(ls.GetTranslatorForPlayer) .. " / " .. tostring(ls.GetCorescriptLocalizations)
out[#out+1] = "rs fn=" .. tostring(info.IsClient)
return table.concat(out, "\n")
