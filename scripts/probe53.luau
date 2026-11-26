-- Replicate getMapCameraCFrame: reads a map model's parts
local rf = game:GetService("ReplicatedFirst").ClassicLoadingScreen
local function names(obj, depth)
  local out = {}
  if depth == 0 then return out end
  for _, c in ipairs(obj:GetChildren()) do
    out[#out+1] = c.Name..":"..c.ClassName
    local sub = names(c, depth-1)
    for _, s in ipairs(sub) do out[#out+1] = c.Name.."/"..s end
  end
  return out
end
local list = names(rf, 2)
return "n="..#list.."\n"..table.concat(list, "\n")
