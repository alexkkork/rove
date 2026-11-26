//! Derive transform aliases from a received canonical frame without keeping
//! stale aliases from an earlier local move.
use super::*;

fn raw(node: &world::Node, key: &str) -> Option<Json> {
    node.local_overrides.get(key).cloned().or_else(|| {
        if let Some(value) = received_joint_property(node, key) { return Some(value); }
        if let Some(projected) = &node.projected {
            projected.property(key).ok().flatten()
        } else {
            node.properties.get(key).cloned()
        }
    })
}
fn revision(node: &world::Node, key: &str) -> u64 {
    if let Some(value) = received_joint_revision(node, key) { return value; }
    node.revisions
        .get(key)
        .copied()
        .or_else(|| {
            node.projected
                .as_ref()
                .and_then(|p| p.revision(key).ok().flatten())
                .and_then(|v| v.as_u64())
        })
        .unwrap_or(0)
}
pub(crate) fn alias(node: &world::Node, key: &str) -> Option<mlua::Result<Json>> {
    if !matches!(key, "Position" | "Orientation" | "Rotation")
        || !world::class_chain(&node.class_name)
            .iter()
            .any(|c| c == "BasePart")
    {
        return None;
    }
    let cf = raw(node, "CFrame")?;
    if raw(node, key).is_some() && revision(node, key) > revision(node, "CFrame") {
        return None;
    }
    // BasePart aliases must always answer: games iterate every part and read
    // `.Position` (loading-screen map framing), and a missing transform in a
    // partial snapshot should yield the pivot's position, not a runtime error.
    Some(match from_frame(&cf, key) {
        Ok(value) => Ok(value),
        Err(_) => Ok(json!({"$vector3":[0.,0.,0.]})),
    })
}
/// Use the same projection for reads and per-action notifications.
pub(crate) fn from_frame(cf: &Json, key: &str) -> mlua::Result<Json> {
    if key == "Position" {
        let p = cf.pointer("/$cframe/position").and_then(Json::as_array);
        if p.is_some_and(|v| {
            v.len() == 3 && v.iter().all(|v| v.as_f64().is_some_and(f64::is_finite))
        }) {
            Ok(json!({"$vector3":p.unwrap()}))
        } else {
            Err(err("part position is unavailable"))
        }
    } else {
        cframe_from_json(cf)
            .map(|frame| json!({"$vector3":euler_angles(frame.rotation,key=="Orientation")}))
    }
}

pub(crate) fn clear_overrides(node: &mut world::Node, key: &str) {
    if key == "CFrame"
        && world::class_chain(&node.class_name)
            .iter()
            .any(|c| c == "BasePart")
    {
        for key in ["CFrame", "Position", "Orientation", "Rotation"] {
            node.local_overrides.remove(key);
        }
    }
}
