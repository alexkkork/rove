local out = {}
local n = 0
for _, d in ipairs(game:GetService("ReplicatedStorage"):GetDescendants()) do
  for _, k in ipairs({"MeshId","SoundId","Texture","Image","AnimationId"}) do
    local ok, v = pcall(function() return d[k] end)
    if ok and typeof(v) == "string" and v ~= "" then
      out[#out+1] = d.ClassName.."."..k.."="..v; n += 1
      break
    end
  end
  if n > 10 then break end
end
return "n="..n.."\n"..table.concat(out, "\n")
