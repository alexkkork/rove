local d = game:GetService("ReplicatedStorage").Database.Sync
for _, name in ipairs({"Item","Pets","Effects","Emotes","Perks","Radios"}) do
  local c = d:FindFirstChild(name)
  print(name, c and c.ClassName, c and tostring(c.Source and #c.Source), c and tostring(select(2, pcall(getscriptbytecode, c))):sub(1,40))
end
local ok, err = pcall(require, d:FindFirstChild("Effects"))
print("req", ok, tostring(err):sub(1,150))
