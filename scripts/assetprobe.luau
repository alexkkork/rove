local out = {}
local lp = game:GetService("Players").LocalPlayer
local n = 0
for _, d in ipairs(workspace:GetDescendants()) do
  if d:IsA("MeshPart") or d:IsA("Sound") or d:IsA("Decal") or d:IsA("Texture") then
    local ref = nil
    for _, k in ipairs({"MeshId","SoundId","Texture"}) do
      local ok, v = pcall(function() return d[k] end)
      if ok and typeof(v) == "string" and v ~= "" then ref = k.."="..v end
    end
    if ref then out[#out+1] = d.ClassName.." "..ref; n += 1 end
    if n > 8 then break end
  end
end
return table.concat(out, "\n")
