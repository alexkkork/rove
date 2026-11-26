local names = {"tick","time","elapsedTime","debug.profilebegin","newproxy","gcinfo","ypcall","wait","delay","spawn","Random.new","Vector2.zero","CFrame.lookAt","table.clone","utf8.len","bit32.band","os.clock","DateTime.now","TweenInfo.new","NumberSequence.new","ColorSequence.new","OverlapParams.new","RaycastParams.new","task.defer"}
local out = {}
for _, n in ipairs(names) do
  local ok, v = pcall(function() return (loadstring or load)("return "..n)() end)
  out[#out+1] = n.."="..tostring(ok and typeof(v) or ("ERR "..tostring(v):sub(1,60)))
end
return table.concat(out, "\n")
