local out = {}
for _, n in ipairs({"Vector2Value","Vector3Value","BoolValue","NumberValue","BinaryStringValue"}) do
  local ok, v = pcall(Instance.new, n)
  out[#out+1] = n.."="..tostring(ok).." "..tostring(v):sub(1,60)
end
return table.concat(out, "\n")
