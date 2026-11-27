local conn = workspace.DescendantAdded:Connect(function(p)
  if p:IsA("BasePart") then
    local ok, mag = pcall(function() return p.Size.Magnitude end)
    if not ok then print("FAIL", p:GetFullName(), p.ClassName, tostring(select(2, pcall(function() return p.Size end)))) end
  end
end)
task.wait(12)
conn:Disconnect()
print("done")
