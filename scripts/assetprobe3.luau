local out = {}
local n=0
local function scan(root,label)
  for _, d in ipairs(root:GetDescendants()) do
    out[#out+1] = label.." "..d.ClassName
    n+=1
    if n>25 then return end
  end
end
scan(workspace,"ws")
return "n="..n.."\n"..table.concat(out,"\n")
