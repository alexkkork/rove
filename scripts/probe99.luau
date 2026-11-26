local Game = game
local ls = Game:GetService("LocalizationService")
print("A", typeof(ls))
local ok, idx = pcall(function() return ls.GetTranslatorForPlayer end)
print("B", ok, typeof(idx))
print("C", ls.ClassName)
