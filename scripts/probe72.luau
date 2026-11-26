local out = {}
for _, root in ipairs({workspace, game:GetService("ReplicatedFirst")}) do
  for _, c in ipairs(root:GetChildren()) do
    out[#out+1] = root.ClassName.."/"..c.Name..":"..c.ClassName
  end
end
return table.concat(out, "\n")
