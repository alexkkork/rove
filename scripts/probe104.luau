local svc = game:GetService("LocalizationService")
return "name="..svc.Name.." parent="..tostring(svc.Parent).." full="..svc:GetFullName()
