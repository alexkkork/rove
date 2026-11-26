local cp = game:GetService("ContentProvider")
local out = {}
local function t(n, f) local r=table.pack(pcall(f)) out[#out+1]=n.."="..tostring(r[1]).." "..tostring(r[2]):sub(1,90) end
t("regDefault", function() cp:RegisterDefaultEncryptionKey(string.rep("ab",32)) end)
t("regAsset", function() cp:RegisterEncryptedAsset(12345, string.rep("cd",32)) end)
t("regB64", function() cp:RegisterEncryptedAsset("rbxassetid://999", "AAAAAAAAAAAAAAAAAAAAAA==") end)
t("list", function() return #cp:ListEncryptedAssets() end)
t("badKey", function() cp:RegisterEncryptedAsset(1, "short") end)
t("status", function() return cp:GetAssetFetchStatus(12345) end)
t("unreg", function() cp:UnregisterEncryptedAsset(12345) return #cp:ListEncryptedAssets() end)
t("failedReqs", function() return #cp:GetFailedRequests() end)
return table.concat(out,"\n")
