local svc = game:GetService("LocalizationService")
local ok, err = pcall(function() return svc:GetTranslatorForPlayer(nil) end)
return tostring(ok).." "..tostring(err):sub(1,200)
