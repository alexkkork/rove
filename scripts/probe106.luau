local svc = game:GetService("LocalizationService")
local names = {"GetTranslatorForPlayer","GetCorescriptLocalizations","GetTableEntries","GetIsLoadingInternalTranslations","StartTextScraper","SetRobloxLocaleId"}
local out = {}
for _, n in ipairs(names) do
  out[#out+1] = n.."="..tostring(pcall(function() return svc[n] end))
end
return table.concat(out, "\n")
