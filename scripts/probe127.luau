local d = game:GetService("ReplicatedStorage").Database.Sync
local out = {}
for _, name in ipairs({"Item","Effects","Emotes","Perks"}) do
  local c = d:FindFirstChild(name)
  local ok, err = pcall(require, c)
  out[#out+1] = name.." req="..tostring(ok).." "..tostring(err):sub(1,110)
end
return table.concat(out, "\n")
