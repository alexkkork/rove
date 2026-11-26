local mod = require(game:GetService("ReplicatedStorage").BrickBattle.Attributes)
local d = workspace:FindFirstChild("ogflatlander")
local out = {}
local function t(n, f) local r=table.pack(pcall(f)) out[#out+1]=n.."="..tostring(r[1]).." "..tostring(r[2]):sub(1,90) end
t("attrs", function() return d:GetAttributes().ToolAnimation end)
t("child", function() local c=d:FindFirstChild("$AttributeToolAnimation") return c and c.ClassName end)
t("childval", function() local c=d:FindFirstChild("$AttributeToolAnimation") return c.Value end)
return table.concat(out, "\n")
