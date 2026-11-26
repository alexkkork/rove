local out = {}
out[#out+1] = "teams="..tostring(game:GetService("Teams").GetTeams)
out[#out+1] = "loc="..tostring(game:GetService("LocalizationService").GetTableEntries)
out[#out+1] = "locClass="..game:GetService("LocalizationService").ClassName
local svc = game:GetService("LocalizationService")
local ok, v = pcall(function() return svc["GetTableEntries"] end)
out[#out+1] = "indexstr="..tostring(ok).." "..tostring(v)
return table.concat(out, "\n")
