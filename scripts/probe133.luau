local ok, src = pcall(decompile, game:GetService("ReplicatedStorage").ClassicOverlays.Leaderboard.Leaderboard)
local lines = ok and string.split(src, "\n") or {}
local out = {"ok="..tostring(ok)}
for i = 1, 40 do
  local l = lines[i] or ""
  if l:find("root") or l:find("panel") then out[#out+1] = i..": "..l end
end
return table.concat(out, "\n")
