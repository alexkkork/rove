-- Exercise RemoteEvent / RemoteFunction from the replicated world
local rs = game:GetService("ReplicatedStorage")
local events, funcs = {}, {}
for _, d in ipairs(rs:GetDescendants()) do
  if d:IsA("RemoteEvent") then events[#events+1] = d end
  if d:IsA("RemoteFunction") then funcs[#funcs+1] = d end
end
print("P1 remotes found events:", #events, "funcs:", #funcs)
if #events > 0 then
  local ok, err = pcall(function() events[1]:FireServer("rove-test") end)
  print("P2 FireServer", ok and "ok" or tostring(err):sub(1, 60))
end
if #funcs > 0 then
  local ok, res = pcall(function() return funcs[1]:InvokeServer("rove-test") end)
  print("P3 InvokeServer", ok and ("ok " .. tostring(res):sub(1, 30)) or tostring(res):sub(1, 60))
end
task.wait(2)
print("P4 done")
