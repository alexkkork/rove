local P = game:GetService("Players")
local out = {}
out[#out+1] = "fn="..typeof(P.GetPlayerFromCharacter)
local lp = P.LocalPlayer
local c = lp.Character
out[#out+1] = "char="..tostring(c)
if c then
  local ok, v = pcall(function() return P:GetPlayerFromCharacter(c) end)
  out[#out+1] = "call="..tostring(ok).." "..tostring(v)
end
return table.concat(out, "\n")
