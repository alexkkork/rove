-- print every key in the issued join data via the protocol surface
local out = {}
local ok, res = pcall(function() return game:GetService("DataStoreService") end)
local g = game
for _, k in ipairs({"JobId","PlaceId","GameId","CreatorId","CreatorType","PrivateServerId","VIPServerId"}) do
  local ok2, v = pcall(function() return g[k] end)
  out[#out+1] = k.."="..tostring(ok2 and v or "ERR")
end
return table.concat(out, "\n")
