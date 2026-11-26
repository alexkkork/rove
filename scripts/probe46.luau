local names = {"GetChildren","GetDescendants","GetPlayers","GetGuiObjectsAtPosition","GetTagged","GetPositionOnScreen","GetBoundingBox","GetPartBoundsInBox","GetPartBoundsInRadius","GetConnectedParts","GetJoints","GetTouchingParts","GetMass","GetClosestPointOnSurface","GetPivot","GetExtentsSize","GetScale","GetAttributeChangedSignal","GetPropertyChangedSignal","GetActor","GetFullName","GetService","FindFirstChild","FindFirstChildWhichIsA","FindFirstAncestor","GetNetworkOwner","GetPhysicsStepId","GetCameraYInvertValue","GetInputBindings","GetInputActions","GetTeams","GetState","GetBoundParts","GetChildren","GetDebugId","GetVertexPosition","GetMeshSize","GetAppliedImpulse"}
local out = {}
for _, n in ipairs(names) do
  local ok = pcall(function() return game:GetService("Workspace")[n] end)
  out[#out+1] = n.."="..tostring(ok)
end
return table.concat(out, "\n")
