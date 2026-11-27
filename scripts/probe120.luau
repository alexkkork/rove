local b
for _, d in ipairs(workspace:GetDescendants()) do if d:IsA("InputBinding") then b = d break end end
if not b then return "none" end
return "full="..b:GetFullName().." raw="..tostring(b:GetAttribute("x")).." kc="..typeof(b.KeyCode)..":"..tostring(b.KeyCode).." kcProp="..tostring((function() local ok,v=pcall(function() return b["KeyCode"] end) return tostring(ok).."/"..typeof(v) end)())
