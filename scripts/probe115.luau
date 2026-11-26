local d = game:GetService("ReplicatedStorage").Database
return "db="..tostring(d).." sync="..tostring(d and d:FindFirstChild("Sync")) .. " cls=" .. tostring(d and d.Sync and d.Sync.ClassName) .. " src=" .. tostring(d and d.Sync and d.Sync.Source and #d.Sync.Source)
