local svc = game:GetService("LocalizationService")
local ok, v = pcall(function() return svc.GetCorescriptLocalizations end)
return tostring(ok).." "..typeof(v).." "..tostring(v):sub(1,120)
