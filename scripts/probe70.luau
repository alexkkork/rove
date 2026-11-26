local s = game:GetService("ReplicatedFirst").ClassicLoadingScreen.ClassicLoadingScreen
local out = {"class="..s.ClassName.." source_len="..tostring(s.Source and #s.Source)}
local ok, d = pcall(function() return decompile(s) end)
out[#out+1] = "decompile="..tostring(ok).." "..tostring(d):sub(1,90)
if ok then
  local lines = string.split(d, "\n")
  for i = 124, 133 do out[#out+1] = i..": "..tostring(lines[i]) end
end
return table.concat(out, "\n")
