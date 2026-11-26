local a = workspace:FindFirstChild("WalkAnim") or workspace:FindFirstChildWhichIsA("Animation")
local out = {"a="..tostring(a and a.Name)}
if a then
  for _, k in ipairs({"AnimationId","AnimationContent"}) do
    local ok, v = pcall(function() return a[k] end)
    out[#out+1] = k.."="..tostring(ok).." "..tostring(v)
  end
  local kids = {}
  for _, c in ipairs(a:GetChildren()) do kids[#kids+1] = c.Name..":"..c.ClassName end
  out[#out+1] = "kids="..table.concat(kids, ",")
end
return table.concat(out, "\n")
