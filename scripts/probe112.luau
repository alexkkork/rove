local out = {}
for _, p in ipairs(game:GetService("Players"):GetPlayers()) do
  local ls = p:FindFirstChild("leaderstats")
  if ls then
    for _, c in ipairs(ls:GetChildren()) do
      local ok, v = pcall(function() return c.Value end)
      out[#out+1] = p.Name.."."..c.Name.." cls="..c.ClassName.." ok="..tostring(ok).." v="..tostring(v).." kids="..#c:GetChildren()
    end
  end
end
return "n="..#out.."\n"..table.concat(out, "\n")
