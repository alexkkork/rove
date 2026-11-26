local out={}
local n=0
for _,d in ipairs(workspace:GetDescendants()) do
  if d:IsA("BasePart") then
    out[#out+1]=d.ClassName.." mat="..tostring(d.Material)
    n+=1
    if n>6 then break end
  end
end
local mesh=workspace:FindFirstChildWhichIsA("MeshPart", true)
out[#out+1]="mesh="..tostring(mesh)
if mesh then
  local ok,v=pcall(function() return mesh.MeshId end)
  out[#out+1]="meshid_ok="..tostring(ok).." v="..tostring(v)
end
return table.concat(out,"\n")
