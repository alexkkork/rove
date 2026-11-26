//! Local movement and model transforms for the Rust-owned Instance world.
//!
//! These calls only create local edits/intents. They never send packets or
//! claim server-authoritative movement.

use super::{
    cframe::CFrameValue,
    values,
    world::{self, WorldRef},
};
use mlua::{
    Lua, MultiValue, Value,
};
use serde_json::{json, Value as Json};
use std::{cell::Cell, rc::Rc};

#[path = "character_velocity.rs"]
mod character_velocity;
#[path = "character_transform.rs"]
mod character_transform;
#[path = "owned_part_transform.rs"]
mod owned_part_transform;
pub(crate) use owned_part_transform::discard_invalid as discard_invalid_owned_part_intents;
#[path = "humanoid_walk_speed.rs"]
mod humanoid_walk_speed;
#[path = "humanoid_walk_target.rs"]
mod humanoid_walk_target;
#[path = "humanoid_move_observation.rs"]
mod humanoid_move_observation;
#[path = "move_to_target.rs"]
mod move_to_target;
pub(crate) use move_to_target::flush as flush_move_to_target;
#[path = "model_pivots.rs"]
mod pivots;
#[path = "part_transform_aliases.rs"]
mod part_aliases;
pub(crate) use part_aliases::{alias as part_transform_alias, clear_overrides as clear_received_part_overrides, from_frame as part_alias_from_frame};
pub(crate) use pivots::bulk_move::replay as replay_bulk_move;
pub(crate) use pivots::joint_poses::{
    animate as apply_animation_poses,
    changed as joint_pose_changed, flush as flush_joint_poses, replay as replay_joint_pose,
    preserve_received as preserve_joint_pose, received as received_joint_poses,
    received_property as received_joint_property, received_revision as received_joint_revision,
    remember_received_motor,
    before_received as before_received_joint_poses,
};
pub(crate) use pivots::attachments::{
    attachment_frame, attachment_origin, clear_received_attachment_overrides,
    clear_received_attachment_alias,
    preserve_attachment_pose, refresh as refresh_attachments, replay_attachment_pose,
    transform_property as attachment_transform_property, watch_signal as watch_attachment_signal,
};
pub(crate) use pivots::scaling::{
    clone_scale_state, export_scale_baseline, import_scale_baseline, preserve_scale, replay_scale,
    ScaleState,
};

fn err(message: impl Into<String>) -> mlua::Error {
    mlua::Error::RuntimeError(message.into())
}
fn vector(value: &Value, label: &str) -> mlua::Result<[f64; 3]> {
    match value {
        Value::Vector(v) => {
            let out: [f64; 3] = [v.x() as f64, v.y() as f64, v.z() as f64];
            if out.iter().all(|x| x.is_finite() && x.abs() <= 1.0e6) {
                Ok(out)
            } else {
                Err(err(format!(
                    "{label} components must be finite and bounded"
                )))
            }
        }
        _ => Err(err(format!("{label} expects a Vector3"))),
    }
}
pub(super) fn json_identity_rotation() -> Json {
    Json::Array(
        [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
            .into_iter()
            .map(Json::from)
            .collect(),
    )
}
pub(super) fn cframe_from_json(value: &Json) -> mlua::Result<CFrameValue> {
    let raw = value
        .get("$cframe")
        .ok_or_else(|| err("CFrame is unavailable in snapshot"))?;
    let p = raw
        .get("position")
        .and_then(Json::as_array)
        .ok_or_else(|| err("CFrame position is unavailable in snapshot"))?;
    let r = match raw.get("rotation").and_then(Json::as_array) {
        Some(r) => r,
        None => {
            // Wire shorthand: rotation_id 2 = identity, 0 = axis-aligned with
            // a packed 6-byte representation.
            let id = raw
                .get("rotation_id")
                .and_then(Json::as_u64)
                .ok_or_else(|| err("CFrame rotation is unavailable in snapshot"))?;
            if id != 2 {
                return Err(err("CFrame rotated shorthand is unsupported"));
            }
            return Ok(CFrameValue::new(
                [p[0].as_f64().unwrap_or(0.0), p[1].as_f64().unwrap_or(0.0), p[2].as_f64().unwrap_or(0.0)],
                [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            ));
        }
    };
    if p.len() != 3 || r.len() != 9 {
        return Err(err("CFrame transform has invalid dimensions"));
    }
    let mut position = [0.; 3];
    let mut rotation = [0.; 9];
    for (dst, src) in position.iter_mut().zip(p) {
        *dst = src
            .as_f64()
            .ok_or_else(|| err("CFrame position is invalid"))?;
    }
    for (dst, src) in rotation.iter_mut().zip(r) {
        *dst = src
            .as_f64()
            .ok_or_else(|| err("CFrame rotation is invalid"))?;
    }
    if !position
        .iter()
        .chain(rotation.iter())
        .all(|x| x.is_finite())
    {
        return Err(err("CFrame components must be finite"));
    }
    Ok(CFrameValue::new(position, rotation))
}
fn cframe_arg(value: &Value) -> mlua::Result<CFrameValue> {
    match value {
        Value::UserData(data) => Ok(data.borrow::<CFrameValue>()?.clone()),
        _ => Err(err("expected a CFrame")),
    }
}
fn encode_frame(cf: &CFrameValue) -> Json {
    json!({"$cframe":{"position":cf.position,"rotation":cf.rotation}})
}
fn euler_angles(rotation: [f64; 9], yxz: bool) -> [f64; 3] {
    let (x, y, z) = if yxz {
        let x = (-rotation[5]).clamp(-1., 1.).asin();
        if x.cos().abs() > 1e-7 {
            (
                x,
                rotation[2].atan2(rotation[8]),
                rotation[3].atan2(rotation[4]),
            )
        } else {
            (x, (-rotation[6]).atan2(rotation[0]), 0.)
        }
    } else {
        let y = rotation[2].clamp(-1., 1.).asin();
        if y.cos().abs() > 1e-7 {
            (
                (-rotation[5]).atan2(rotation[8]),
                y,
                (-rotation[1]).atan2(rotation[0]),
            )
        } else {
            (rotation[7].atan2(rotation[4]), y, 0.)
        }
    };
    [x.to_degrees(), y.to_degrees(), z.to_degrees()]
}
fn part_fields(cf: &CFrameValue) -> [(&'static str, Json); 4] {
    [
        ("CFrame", encode_frame(cf)),
        ("Position", json!({"$vector3":cf.position})),
        (
            "Orientation",
            json!({"$vector3":euler_angles(cf.rotation,true)}),
        ),
        (
            "Rotation",
            json!({"$vector3":euler_angles(cf.rotation,false)}),
        ),
    ]
}
fn compose(a: &CFrameValue, b: &CFrameValue) -> CFrameValue {
    let mut p = [0.; 3];
    let mut r = [0.; 9];
    for i in 0..3 {
        p[i] = a.position[i]
            + (0..3)
                .map(|j| a.rotation[i * 3 + j] * b.position[j])
                .sum::<f64>();
    }
    for i in 0..3 {
        for j in 0..3 {
            r[i * 3 + j] = (0..3)
                .map(|k| a.rotation[i * 3 + k] * b.rotation[k * 3 + j])
                .sum();
        }
    }
    CFrameValue::new(p, r)
}
fn inverse(a: &CFrameValue) -> CFrameValue {
    let mut r = [0.; 9];
    for i in 0..3 {
        for j in 0..3 {
            r[i * 3 + j] = a.rotation[j * 3 + i];
        }
    }
    let mut p = [0.; 3];
    for i in 0..3 {
        p[i] = -(0..3).map(|j| r[i * 3 + j] * a.position[j]).sum::<f64>();
    }
    CFrameValue::new(p, r)
}
fn class(ctx: &WorldRef, id: &str) -> mlua::Result<String> {
    ctx.borrow()
        .nodes
        .get(id)
        .map(|n| n.class_name.clone())
        .ok_or_else(|| err("instance is not in this world"))
}
fn is_basepart(ctx: &WorldRef, id: &str) -> bool {
    world::is_a(ctx, id, "BasePart")
}
struct IntentSequence(Cell<u64>, Cell<usize>, Rc<Cell<usize>>);
pub(crate) struct IntentReplayGuard(Rc<Cell<usize>>);
impl Drop for IntentReplayGuard { fn drop(&mut self) { self.0.set(self.0.get()-1); } }
pub(crate) fn suppress_replayed_intents(lua: &Lua) -> Option<IntentReplayGuard> {
    let state=lua.app_data_ref::<IntentSequence>()?;
    state.2.set(state.2.get()+1);
    Some(IntentReplayGuard(state.2.clone()))
}
fn replaying_intents(lua: &Lua) -> bool {
    lua.app_data_ref::<IntentSequence>().is_some_and(|s|s.2.get()>0)
}
pub(crate) fn remaining_intents(lua: &Lua, ctx: &WorldRef) -> usize {
    let limit=lua.app_data_ref::<IntentSequence>().map_or(0,|s|s.1.get());
    limit.saturating_sub(ctx.borrow().effects.get("movement_intents").and_then(Json::as_array).map_or(0,Vec::len))
}
pub(crate) fn limit_actor_intents(lua: &Lua, remaining: usize) {
    if let Some(state)=lua.app_data_ref::<IntentSequence>() {state.1.set(remaining.min(256));}
}
pub(crate) fn forward_actor_intent(lua: &Lua,ctx: &WorldRef,value: Json,cursor: usize) -> mlua::Result<()> {
    queue_intent(lua,ctx,value)?;
    ctx.borrow_mut().effects["movement_intents"].as_array_mut().unwrap().last_mut().unwrap()["mutation_cursor"]=json!(cursor);
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HumanoidStateItem {
    name: &'static str,
    value: i32,
}
pub(super) const HUMANOID_STATES: [(&str, i32); 17] = [
    ("FallingDown", 0),
    ("Ragdoll", 1),
    ("GettingUp", 2),
    ("Jumping", 3),
    ("Swimming", 4),
    ("Freefall", 5),
    ("Flying", 6),
    ("Landed", 7),
    ("Running", 8),
    ("RunningNoPhysics", 10),
    ("StrafingNoPhysics", 11),
    ("Climbing", 12),
    ("Seated", 13),
    ("PlatformStanding", 14),
    ("Dead", 15),
    ("Physics", 16),
    ("None", 18),
];
pub(super) fn state_item(lua: &Lua, value: i32) -> mlua::Result<Value> {
    let name = HUMANOID_STATES
        .iter()
        .find(|(_, v)| *v == value)
        .map(|(n, _)| *n)
        .ok_or_else(|| err("invalid HumanoidStateType"))?;
    Ok(Value::UserData(
        super::enums::enum_item(lua,"HumanoidStateType",name)?,
    ))
}
fn state_arg(value: &Value) -> mlua::Result<HumanoidStateItem> {
    let Value::UserData(item) = value else {
        return Err(err("expected HumanoidStateType"));
    };
    if super::enums::enum_item_matches(item, "HumanoidStateType").unwrap_or(false) {
        let value = super::enums::enum_item_value(item)?;
        let name = HUMANOID_STATES
            .iter()
            .find(|(_, v)| *v == value)
            .map(|(n, _)| *n)
            .ok_or_else(|| err("invalid HumanoidStateType"))?;
        return Ok(HumanoidStateItem { name, value });
    }
    Err(err("expected Enum.HumanoidStateType item"))
}

fn property(ctx: &WorldRef, id: &str, name: &str) -> mlua::Result<Json> {
    world::get_property(ctx, id, name)
}
fn store_frame(lua: &Lua, ctx: &WorldRef, id: &str, cf: CFrameValue) -> mlua::Result<()> {
    let fields = part_fields(&cf)
        .into_iter()
        .map(|(key, value)| (key.into(), value))
        .collect();
    let changes = world::apply_pose_batch(
        ctx,
        &[world::PoseWrite {
            id: id.into(),
            fields,
        }],
        false,
    )?;
    world::pose_events(lua, ctx, changes)
}

fn set_part_transform(
    lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    key: &str,
    value: Value,
) -> mlua::Result<bool> {
    if !is_basepart(ctx, id) || !matches!(key, "CFrame" | "Position" | "Orientation" | "Rotation") {
        return Ok(false);
    }
    let mut frame = if key == "CFrame" {
        cframe_arg(&value)?
    } else {
        cframe_from_json(&property(ctx, id, "CFrame")?)?
    };
    match key {
        "CFrame" => {}
        "Position" => frame.position = vector(&value, "Position")?,
        "Orientation" | "Rotation" => {
            let mut a = vector(&value, key)?;
            for value in &mut a {
                *value = value.to_radians();
            }
            let (sx, cx) = (a[0].sin(), a[0].cos());
            let (sy, cy) = (a[1].sin(), a[1].cos());
            let (sz, cz) = (a[2].sin(), a[2].cos());
            let rx = [1., 0., 0., 0., cx, -sx, 0., sx, cx];
            let ry = [cy, 0., sy, 0., 1., 0., -sy, 0., cy];
            let rz = [cz, -sz, 0., sz, cz, 0., 0., 0., 1.];
            let product = |l: [f64; 9], r: [f64; 9]| {
                let mut out = [0.; 9];
                for i in 0..3 {
                    for j in 0..3 {
                        for k in 0..3 {
                            out[i * 3 + j] += l[i * 3 + k] * r[k * 3 + j];
                        }
                    }
                }
                out
            };
            frame.rotation = if key == "Orientation" {
                product(product(ry, rx), rz)
            } else {
                product(product(rx, ry), rz)
            };
        }
        _ => unreachable!(),
    }
    let handled = if key == "CFrame" {
        pivots::bulk_move::move_rigid_frame(lua, ctx, id, &frame)?
    } else {
        pivots::joint_poses::rebase(lua, ctx, id, &frame)?
    };
    if !handled {
        let intent = character_transform::prepare(lua, ctx, id, &frame)?;
        store_frame(lua, ctx, id, frame)?;
        if let Some(intent) = intent { queue_intent(lua, ctx, intent)?; }
    }
    Ok(true)
}
fn set_cframe(lua: &Lua, ctx: &WorldRef, id: &str, value: Value) -> mlua::Result<bool> {
    set_part_transform(lua, ctx, id, "CFrame", value)
}
fn set_position(lua: &Lua, ctx: &WorldRef, id: &str, value: Value) -> mlua::Result<bool> {
    set_part_transform(lua, ctx, id, "Position", value)
}
fn set_orientation(lua: &Lua, ctx: &WorldRef, id: &str, value: Value) -> mlua::Result<bool> {
    set_part_transform(lua, ctx, id, "Orientation", value)
}
fn set_rotation(lua: &Lua, ctx: &WorldRef, id: &str, value: Value) -> mlua::Result<bool> {
    set_part_transform(lua, ctx, id, "Rotation", value)
}

fn validate_intent_queue(lua: &Lua, ctx: &WorldRef) -> mlua::Result<()> {
    if replaying_intents(lua) { return Ok(()); }
    let world=ctx.borrow();
    let limit=lua.app_data_ref::<IntentSequence>().map_or(0,|s|s.1.get());
    if limit==0 || world.effects.get("movement_intents").is_some_and(|v|v.as_array().is_none_or(|rows|rows.len()>=limit)) {
        return Err(err("local movement intent queue is full or invalid"));
    }
    let counter=lua.app_data_ref::<IntentSequence>().ok_or_else(||err("movement intent sequence is not installed"))?;
    counter.0.get().checked_add(1).ok_or_else(||err("movement intent sequence exhausted"))?;
    Ok(())
}
fn queue_intent(lua: &Lua, ctx: &WorldRef, mut value: Json) -> mlua::Result<()> {
    if replaying_intents(lua) { return Ok(()); }
    validate_intent_queue(lua,ctx)?;
    let observation = humanoid_move_observation::from_intent(ctx, &value)?;
    let mut world=ctx.borrow_mut();
    let counter=lua.app_data_ref::<IntentSequence>().unwrap();
    let sequence=counter.0.get()+1;
    counter.0.set(sequence);
    let mutation_cursor = world
        .effects
        .get("mutations")
        .and_then(Json::as_array)
        .map_or(0, |items| items.len());
    value["sequence"] = json!(sequence);
    value["mutation_cursor"] = json!(mutation_cursor);
    let intents = world
        .effects
        .entry("movement_intents")
        .or_insert_with(|| Json::Array(vec![]));
    let array = intents
        .as_array_mut()
        .ok_or_else(|| err("movement intent channel is invalid"))?;
    let target_control = matches!(value["kind"].as_str(), Some("MoveTo" | "Move" | "CancelMoveTo"))
        .then(|| value.clone());
    array.push(value);
    drop(world);
    if let Some(value) = target_control { move_to_target::accepted(lua, ctx, &value, sequence); }
    if let Some((id, input)) = observation { humanoid_move_observation::apply(lua, ctx, &id, input, sequence)?; }
    Ok(())
}

fn humanoid_move(
    _lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    args: MultiValue,
) -> mlua::Result<MultiValue> {
    let receiver = class(ctx, id)?;
    if receiver != "Humanoid" && receiver != "Player" {
        return Err(err(
            "movement method requires a Humanoid or Player receiver",
        ));
    }
    let args: Vec<Value> = args.into_iter().collect();
    if args.is_empty() || args.len() > 2 {
        return Err(err(
            "Move expects a direction and optional camera-relative flag",
        ));
    }
    let direction = vector(&args[0], "Move")?;
    let relative = match args.get(1) {
        None | Some(Value::Boolean(false)) => false,
        Some(Value::Boolean(true)) => true,
        _ => return Err(err("Move camera-relative argument must be a boolean")),
    };
    let mut intent =
        json!({"kind":"Move","instance_id":id,"vector":direction,"relative_to_camera":relative});
    if relative {
        let workspace = ctx.borrow().services.get("Workspace").cloned();
        if let Some(workspace) = workspace {
            if let Ok(camera_ref) = world::get_property(ctx, &workspace, "CurrentCamera") {
                if let Some(camera_id) = camera_ref.get("$ref").and_then(Json::as_str) {
                    if let Ok(frame) = world::get_property(ctx, camera_id, "CFrame") {
                        if let Some(rotation) = frame.get("$cframe").and_then(|v| v.get("rotation"))
                        {
                            let received_revision = world::get_property_revision(ctx, camera_id, "CFrame").is_some();
                            let origin = ctx
                                .borrow()
                                .nodes
                                .get(camera_id)
                                .map(|n| {
                                    if n.local_overrides.contains_key("CFrame") {
                                        "local"
                                    } else if received_revision {
                                        "received"
                                    } else {
                                        "class_default"
                                    }
                                })
                                .unwrap_or("unavailable");
                            intent["camera"] = json!({"instance_id":camera_id,"rotation":rotation,"origin":origin});
                        }
                    }
                }
            }
        }
        if intent.get("camera").is_none() {
            intent["camera"] = json!({"instance_id":null,"rotation":[1.,0.,0.,0.,1.,0.,0.,0.,1.],"origin":"class_default"});
        }
    }
    queue_intent(_lua, ctx, intent)?;
    Ok(MultiValue::new())
}
fn humanoid_move_to(
    _lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    args: MultiValue,
) -> mlua::Result<MultiValue> {
    let args: Vec<Value> = args.into_iter().collect();
    if args.is_empty() || args.len() > 2 {
        return Err(err("MoveTo expects a target Vector3 and an optional BasePart"));
    }
    let target = vector(&args[0], "MoveTo")?;
    let part = humanoid_walk_target::part(ctx, args.get(1).unwrap_or(&Value::Nil))?;
    validate_intent_queue(_lua, ctx)?;
    world::set_property(_lua, ctx, id, "WalkToPoint", json!({"$vector3":target}))?;
    world::set_property(_lua, ctx, id, "WalkToPart",
        part.as_ref().map_or(Json::Null, |part| json!({"$ref":part})))?;
    let mut intent = json!({"kind":"MoveTo","instance_id":id,"vector":target});
    if let Some(part) = part { intent["part_id"] = json!(part); }
    queue_intent(
        _lua,
        ctx,
        intent,
    )?;
    Ok(MultiValue::new())
}

fn humanoid_get_state(
    lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    args: MultiValue,
) -> mlua::Result<MultiValue> {
    if !args.is_empty() {
        return Err(err("GetState expects no arguments"));
    }
    if class(ctx, id)? != "Humanoid" {
        return Err(err("Humanoid receiver required"));
    }
    let value = super::humanoid_state::observed(ctx,id)?
        .ok_or_else(||err("Humanoid state is unavailable until received state or local physics feedback exists"))?;
    let mut out = MultiValue::new();
    out.push_back(state_item(lua, value)?);
    Ok(out)
}
fn humanoid_get_state_enabled(
    _lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    args: MultiValue,
) -> mlua::Result<MultiValue> {
    if class(ctx, id)? != "Humanoid" {
        return Err(err("Humanoid receiver required"));
    }
    let args: Vec<Value> = args.into_iter().collect();
    if args.len() != 1 {
        return Err(err("GetStateEnabled expects a HumanoidStateType"));
    }
    let state = state_arg(&args[0])?;
    let key = format!("__HumanoidStateEnabled_{}", state.value);
    let enabled = ctx
        .borrow()
        .nodes
        .get(id)
        .and_then(|n| n.local_overrides.get(&key))
        .and_then(Json::as_bool)
        .unwrap_or(true);
    Ok(MultiValue::from_vec(vec![Value::Boolean(enabled)]))
}
fn humanoid_change_state(
    _lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    args: MultiValue,
) -> mlua::Result<MultiValue> {
    if class(ctx, id)? != "Humanoid" {
        return Err(err("Humanoid receiver required"));
    }
    let args: Vec<Value> = args.into_iter().collect();
    if args.len() != 1 {
        return Err(err("ChangeState expects a HumanoidStateType"));
    }
    let state = state_arg(&args[0])?;
    let node = ctx
        .borrow()
        .nodes
        .get(id)
        .cloned()
        .ok_or_else(|| err("Humanoid is unavailable"))?;
    if node.destroyed {
        return Err(err("cannot change state on a destroyed Humanoid"));
    }
    let key = format!("__HumanoidStateEnabled_{}", state.value);
    if node.local_overrides.get(&key).and_then(Json::as_bool) == Some(false) {
        return Ok(MultiValue::new());
    }
    if world::get_property(ctx, id, "Health")
        .ok()
        .and_then(|v| v.as_f64())
        .is_some_and(|health| health <= 0.)
        && state.value != 15
    {
        return Ok(MultiValue::from_vec(vec![Value::Nil]));
    }
    queue_intent(
        _lua,
        ctx,
        json!({"kind":"ChangeState","instance_id":id,"state":state.name,"state_value":state.value}),
    )?;
    Ok(MultiValue::new())
}
fn humanoid_set_state_enabled(
    lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    args: MultiValue,
) -> mlua::Result<MultiValue> {
    if class(ctx, id)? != "Humanoid" {
        return Err(err("Humanoid receiver required"));
    }
    let args: Vec<Value> = args.into_iter().collect();
    if args.len() != 2 {
        return Err(err(
            "SetStateEnabled expects a HumanoidStateType and boolean",
        ));
    }
    let state = state_arg(&args[0])?;
    let Value::Boolean(enabled) = args[1] else {
        return Err(err("SetStateEnabled expects a boolean"));
    };
    if ctx.borrow().nodes.get(id).is_none_or(|n| n.destroyed) {
        return Err(err("cannot set state enabled on a destroyed Humanoid"));
    }
    let key = format!("__HumanoidStateEnabled_{}", state.value);
    queue_intent(
        lua,
        ctx,
        json!({"kind":"SetStateEnabled","instance_id":id,"state":state.name,"state_value":state.value,"enabled":enabled}),
    )?;
    let previous=ctx.borrow().nodes.get(id).and_then(|n|n.local_overrides.get(&key)).and_then(Json::as_bool).unwrap_or(true);
    world::set_engine_property(ctx, id, &key, json!(enabled), None)?;
    if previous!=enabled {
        super::signals::fire(lua,ctx,id,"StateEnabledChanged",MultiValue::from_vec(vec![state_item(lua,state.value)?,Value::Boolean(enabled)]))?;
    }
    Ok(MultiValue::new())
}
fn set_humanoid_pose_bool(
    lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    key: &str,
    value: Value,
) -> mlua::Result<bool> {
    if class(ctx, id)? != "Humanoid" || !matches!(key, "Sit" | "PlatformStand") {
        return Ok(false);
    }
    let Value::Boolean(value) = value else {
        return Err(err(format!("{key} must be a boolean")));
    };
    world::set_property(lua, ctx, id, key, json!(value))?;
    Ok(true)
}
fn get_humanoid_pose_bool(
    _lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    key: &str,
) -> mlua::Result<Option<Value>> {
    if class(ctx, id)? != "Humanoid" || !matches!(key, "Sit" | "PlatformStand") {
        return Ok(None);
    }
    Ok(Some(Value::Boolean(
        world::get_property(ctx, id, key)
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    )))
}
fn get_humanoid_state_signal(
    lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    key: &str,
) -> mlua::Result<Option<Value>> {
    if class(ctx, id)? != "Humanoid" || !matches!(key, "StateChanged" | "Jumping" | "FreeFalling" | "Running" | "FallingDown" | "GettingUp" | "Ragdoll" | "PlatformStanding" | "Strafing" | "StateEnabledChanged" | "MoveToFinished") {
        return Ok(None);
    }
    Ok(Some(super::signals::signal(lua, ctx, id, key)?))
}

fn set_gravity(lua: &Lua, ctx: &WorldRef, id: &str, value: Value) -> mlua::Result<bool> {
    if class(ctx, id)? != "Workspace" {
        return Ok(false);
    }
    let gravity = match value {
        Value::Number(n) => n,
        Value::Integer(n) => n as f64,
        _ => return Err(err("Gravity must be a number")),
    };
    if !gravity.is_finite() || !(0.0..=crate::live_physics::MAX_LOCAL_GRAVITY).contains(&gravity) {
        return Err(err("Gravity must be finite and between 0 and 1000000"));
    }
    validate_intent_queue(lua, ctx)?;
    let received_sequence = ctx.borrow().sequence;
    world::set_property(lua, ctx, id, "Gravity", json!(gravity))?;
    queue_intent(lua, ctx, json!({"kind":"Gravity","instance_id":id,
        "value":gravity,"received_sequence":received_sequence}))?;
    Ok(true)
}
fn get_gravity(lua: &Lua, ctx: &WorldRef, id: &str) -> mlua::Result<Option<Value>> {
    if class(ctx, id)? != "Workspace" {
        return Ok(None);
    }
    let json = world::get_property(ctx, id, "Gravity")?;
    Ok(Some(values::from_json(lua, &json)?))
}
fn set_anchored(lua: &Lua, ctx: &WorldRef, id: &str, value: Value) -> mlua::Result<bool> {
    if !is_basepart(ctx, id) {
        return Ok(false);
    }
    if !matches!(value, Value::Boolean(_)) {
        return Err(err("Anchored must be a boolean"));
    }
    let previous = property(ctx, id, "Anchored")?;
    if previous.as_bool().is_none() {
        return Err(err("Anchored state is unavailable in snapshot"));
    }
    world::set_property(lua, ctx, id, "Anchored", values::to_json(lua, &value)?)?;
    Ok(true)
}


fn set_humanoid_jump(lua: &Lua, ctx: &WorldRef, id: &str, value: Value) -> mlua::Result<bool> {
    if class(ctx, id)? != "Humanoid" {
        return Ok(false);
    }
    let Value::Boolean(requested) = value else {
        return Err(err("Jump must be a boolean"));
    };
    if requested { validate_intent_queue(lua,ctx)?; }
    let previous = world::get_property(ctx, id, "Jump").ok();
    if previous.as_ref().and_then(Json::as_bool) != Some(requested) {
        world::set_property(lua, ctx, id, "Jump", json!(requested))?;
    }
    if requested {
        let mut settings = Json::Object(Default::default());
        for name in [
            "UseJumpPower",
            "JumpPower",
            "JumpHeight",
            "Health",
            "PlatformStand",
            "Sit",
        ] {
            if let Ok(value) = world::get_property(ctx, id, name) {
                settings[name] = value;
            }
        }
        queue_intent(
            lua,
            ctx,
            json!({"kind":"Jump","instance_id":id,"jump_settings":settings}),
        )?;
    }
    Ok(true)
}

/// Installs local Humanoid movement, Model movement, transform writes, and
/// Workspace.Gravity / BasePart.Anchored property behavior.
pub fn install(lua: &Lua, ctx: &WorldRef) -> mlua::Result<()> {
    if lua.app_data_ref::<IntentSequence>().is_none() {
        lua.set_app_data(IntentSequence(Cell::new(0),Cell::new(256),Rc::new(Cell::new(0))));
        move_to_target::install(lua);
    }
    world::register_method(ctx, "Humanoid", "Move", humanoid_move);
    world::register_method(ctx, "Humanoid", "MoveTo", humanoid_move_to);
    world::register_setter(ctx, "WalkSpeed", humanoid_walk_speed::set);
    world::register_setter(ctx, "WalkToPoint", humanoid_walk_target::point);
    world::register_setter(ctx, "WalkToPart", humanoid_walk_target::target_part);
    humanoid_move_observation::install(ctx);
    world::register_method(ctx, "Humanoid", "GetState", humanoid_get_state);
    world::register_method(
        ctx,
        "Humanoid",
        "GetStateEnabled",
        humanoid_get_state_enabled,
    );
    world::register_method(ctx, "Humanoid", "ChangeState", humanoid_change_state);
    world::register_method(
        ctx,
        "Humanoid",
        "SetStateEnabled",
        humanoid_set_state_enabled,
    );
    world::register_method(ctx, "Player", "Move", humanoid_move);
    pivots::install(ctx);
    pivots::joint_poses::install(lua, ctx)?;
    world::register_setter(ctx, "Anchored", set_anchored);
    world::register_setter(ctx, "CFrame", set_cframe);
    world::register_setter(ctx, "Position", set_position);
    world::register_setter(ctx, "Orientation", set_orientation);
    world::register_setter(ctx, "Rotation", set_rotation);
    super::humanoid_health::install(ctx);
    world::register_setter(ctx, "Jump", set_humanoid_jump);
    world::register_setter(ctx, "Sit", |lua, ctx, id, value| {
        set_humanoid_pose_bool(lua, ctx, id, "Sit", value)
    });
    world::register_setter(ctx, "PlatformStand", |lua, ctx, id, value| {
        set_humanoid_pose_bool(lua, ctx, id, "PlatformStand", value)
    });
    world::register_keyed_getter(ctx, "Sit", get_humanoid_pose_bool);
    world::register_keyed_getter(ctx, "PlatformStand", get_humanoid_pose_bool);
    for key in ["StateChanged","Jumping","FreeFalling","Running","FallingDown","GettingUp","Ragdoll","PlatformStanding","Strafing","StateEnabledChanged","MoveToFinished"] {
        world::register_keyed_getter(ctx,key,get_humanoid_state_signal);
    }
    world::register_setter(ctx, "AssemblyLinearVelocity", character_velocity::set);
    world::register_setter(ctx, "Gravity", set_gravity);
    world::register_getter(ctx, "Gravity", get_gravity);
    // Script settings reach detached prediction through ordered commands.
    let _ = lua;
    Ok(())
}

/// Applies an engine pose command using the existing C++-compatible shape.
/// Returns the normal status JSON so the session layer can report stale IDs.
pub fn apply_engine_poses(ctx: &WorldRef, batch: &Json) -> Result<Json, String> {
    let invalid = || "invalid engine pose batch".to_owned();
    if batch.as_object().is_none_or(|o| {
        o.len() != 3
            || !o.contains_key("character_id")
            || !o.contains_key("root_id")
            || !o.contains_key("frames")
    }) {
        return Err(invalid());
    }
    let character = batch["character_id"].as_str().ok_or_else(invalid)?;
    let root = batch["root_id"].as_str().ok_or_else(invalid)?;
    let frames = batch["frames"].as_array().ok_or_else(invalid)?;
    if character.is_empty() || root.is_empty() || frames.is_empty() || frames.len() > 32 {
        return Err("engine pose batch exceeds limits".into());
    }
    let local_player = ctx
        .borrow()
        .snapshot
        .get("local_player")
        .and_then(Json::as_str)
        .map(str::to_owned);
    let Some(local_player) = local_player else {
        return Ok(json!({"status":"stale_character","frames_applied":0}));
    };
    if world::get_property(ctx, &local_player, "Character")
        .ok()
        .and_then(|v| v.get("$ref").and_then(Json::as_str).map(str::to_owned))
        .as_deref()
        != Some(character)
    {
        return Ok(json!({"status":"stale_character","frames_applied":0}));
    }
    if class(ctx, character).map_err(|e| e.to_string())? != "Model" {
        return Err("engine pose Character is unavailable".into());
    }
    let root_node = ctx.borrow().nodes.get(root).cloned();
    let primary = world::get_property(ctx, character, "PrimaryPart")
        .ok()
        .and_then(|v| v.get("$ref").and_then(Json::as_str).map(str::to_owned));
    if root_node.as_ref().is_none_or(|n| {
        n.destroyed
            || (n.name != "HumanoidRootPart" && primary.as_deref() != Some(root))
            || !is_basepart(ctx, root)
            || !is_descendant_by_parent(ctx, root, character)
    }) {
        return Err("engine pose root is not HumanoidRootPart or Model.PrimaryPart".into());
    }
    let mut seen = std::collections::HashSet::new();
    let mut writes = Vec::new();
    let mut root_included = false;
    for frame in frames {
        let id = frame.get("id").and_then(Json::as_str).ok_or_else(invalid)?;
        let cf = frame.get("cframe").ok_or_else(invalid)?;
        if id.is_empty() || !seen.insert(id.to_owned()) {
            return Err("duplicate or empty engine pose part id".into());
        }
        if frame.as_object().is_none_or(|o| o.len() != 2) {
            return Err(invalid());
        }
        let decoded = cframe_from_json(cf).map_err(|e| e.to_string())?;
        let node = ctx
            .borrow()
            .nodes
            .get(id)
            .cloned()
            .ok_or_else(|| "engine pose frame target is unavailable".to_owned())?;
        if node.destroyed || !is_descendant_by_parent(ctx, id, character) || !is_basepart(ctx, id) {
            return Err("engine pose frame target is not a live Character BasePart".into());
        }
        root_included |= id == root;
        writes.push((id.to_owned(), decoded));
    }
    if !root_included {
        return Err("engine pose batch is missing its root frame".into());
    }
    // Validate the entire batch before committing any part.
    for (id, frame) in &writes {
        for (key, value) in part_fields(frame) {
            world::set_engine_property(ctx, id, key, value, None).map_err(|e| e.to_string())?;
        }
    }
    Ok(json!({"status":"applied","frames_applied":writes.len()}))
}

/// Session-facing variant that forwards physics observations to Actor VMs.
pub fn apply_engine_poses_with_lua(lua: &Lua, ctx: &WorldRef, batch: &Json) -> mlua::Result<Json> {
    pose_events::apply(lua, ctx, batch, pose_events::POSE_KEYS, None, || apply_engine_poses(ctx, batch))
}
fn is_descendant_by_parent(ctx: &WorldRef, id: &str, ancestor: &str) -> bool {
    let world = ctx.borrow();
    let mut current = world.nodes.get(id).and_then(|n| n.parent.clone());
    let mut hops = 0;
    while let Some(value) = current {
        if value == ancestor {
            return true;
        }
        current = world.nodes.get(&value).and_then(|n| n.parent.clone());
        hops += 1;
        if hops > 100_000 {
            return false;
        }
    }
    false
}

/// Accepts owned-part feedback only when its captured source revision remains current.
/// This is intentionally local-only and never updates received properties.
pub fn apply_engine_owned_parts(ctx: &WorldRef, batch: &Json, scope: u32) -> Result<Json, String> {
    if batch.as_object().is_none_or(|m| {
        m.len() != 3
            || !m.contains_key("scope")
            || !m.contains_key("source_revision")
            || !m.contains_key("frames")
    }) || scope == 0
        || scope == u32::MAX
        || batch.get("scope").and_then(Json::as_u64) != Some(scope as u64)
    {
        return Ok(json!({"status":"stale_authority","frames_applied":0}));
    }
    let revision = batch
        .get("source_revision")
        .and_then(Json::as_u64)
        .ok_or("owned-part source_revision is missing")?;
    let frames = batch
        .get("frames")
        .and_then(Json::as_array)
        .ok_or("owned-part frames are missing")?;
    if revision > 9_007_199_254_740_991
        || revision > ctx.borrow().sequence
        || frames.is_empty()
        || frames.len() > 32
    {
        return Ok(json!({"status":"stale_authority","frames_applied":0}));
    }
    let mut seen = std::collections::HashSet::new();
    let mut staged = Vec::new();
    for frame in frames {
        if frame.as_object().is_none_or(|m| {
            m.len() != 4
                || !m.contains_key("id")
                || !m.contains_key("cframe")
                || !m.contains_key("linear_velocity")
                || !m.contains_key("angular_velocity")
        }) {
            return Err("invalid owned-part engine frame".into());
        }
        let id = frame
            .get("id")
            .and_then(Json::as_str)
            .ok_or("owned-part frame id is missing")?;
        if id.is_empty() || id.len() > 256 || !seen.insert(id.to_owned()) {
            return Err("duplicate owned-part engine id".into());
        }
        let decoded = cframe_from_json(frame.get("cframe").ok_or("owned-part CFrame is missing")?)
            .map_err(|e| e.to_string())?;
        if frame["cframe"].as_object().is_none_or(|m| m.len() != 1)
            || frame["cframe"]["$cframe"].as_object().is_none_or(|m| {
                m.len() != 2 || !m.contains_key("position") || !m.contains_key("rotation")
            })
        {
            return Err("owned-part engine requires canonical CFrame".into());
        }
        let linear = frame
            .get("linear_velocity")
            .ok_or("owned-part linear velocity is missing")?;
        let angular = frame
            .get("angular_velocity")
            .ok_or("owned-part angular velocity is missing")?;
        for v in [linear, angular] {
            let a = v.as_array().ok_or("owned-part velocity must be an array")?;
            if a.len() != 3
                || a.iter()
                    .any(|n| n.as_f64().is_none_or(|x| !x.is_finite() || x.abs() >= 1e9))
            {
                return Err("invalid owned-part velocity".into());
            }
        }
        for key in [
            "CFrame",
            "ReplicationPV",
            "Parent",
            "NetworkOwnerV3",
            "Anchored",
            "PhysicsRepRootPart",
            "PhysicsRepRootRef",
        ] {
            if world::get_property_revision(ctx, id, key).is_some_and(|v| v > revision) {
                return Ok(json!({"status":"stale_authority","frames_applied":0}));
            }
        }
        let world = ctx.borrow();
        let node = world
            .nodes
            .get(id)
            .ok_or("owned-part target is unavailable")?;
        if node.destroyed || !node.server_owned || node.class_name != "Part" {
            return Ok(json!({"status":"stale_authority","frames_applied":0}));
        }
        // Authority fields can live in a shared received record. Read them
        // through the same overlay/projection path as ordinary world queries.
        if world::node_property(node, "Anchored")
            .and_then(|v| v.as_bool())
            == Some(true)
        {
            return Ok(json!({"status":"stale_authority","frames_applied":0}));
        }
        for key in ["PhysicsRepRootPart", "PhysicsRepRootRef"] {
            if let Some(value) = world::node_property(node, key) {
                if !value.is_null() && value.get("$ref").and_then(Json::as_str) != Some(id) {
                    return Ok(json!({"status":"stale_authority","frames_applied":0}));
                }
            }
        }
        if world::node_property(node, "NetworkOwnerV3")
            .and_then(|v| v.as_u64())
            != Some(scope as u64)
        {
            return Ok(json!({"status":"stale_authority","frames_applied":0}));
        }
        // The isolated body channel must never overwrite a character or a
        // body connected into a joint/constraint assembly.
        let mut cursor = Some(id.to_owned());
        let mut workspace_ancestor = false;
        let mut in_character = false;
        let mut hops = 0;
        while let Some(current) = cursor {
            let Some(ancestor) = world.nodes.get(&current) else {
                break;
            };
            if ancestor.class_name == "Workspace" {
                workspace_ancestor = true;
                break;
            }
            if world.nodes.of_class("Player").any(|player| {
                world::node_property(player, "Character")
                    .is_some_and(|v| v.get("$ref").and_then(Json::as_str) == Some(current.as_str()))
            }) {
                in_character = true;
            }
            cursor = ancestor.parent.clone();
            hops += 1;
            if hops > 128 {
                break;
            }
        }
        if !workspace_ancestor || in_character {
            return Ok(json!({"status":"stale_authority","frames_applied":0}));
        }
        let connected = world.nodes.matching_class(|class| class.ends_with("Constraint") || class.starts_with("Body")
            || matches!(class, "Motor6D" | "Weld" | "ManualWeld" | "Snap" | "Glue" | "Rotate" | "RotateP" | "RotateV")).any(|other| {
            if other.destroyed {
                return false;
            }
            let part_ref = |key: &str| {
                world::node_property(other, key).and_then(|v| {
                    v.get("$ref").and_then(Json::as_str).map(str::to_owned)
                })
            };
            if matches!(
                other.class_name.as_str(),
                "WeldConstraint"
                    | "Motor6D"
                    | "Weld"
                    | "ManualWeld"
                    | "Snap"
                    | "Glue"
                    | "Rotate"
                    | "RotateP"
                    | "RotateV"
            ) && ["Part0", "Part1"]
                .iter()
                .any(|key| part_ref(key).as_deref() == Some(id))
            {
                return true;
            }
            if other.class_name.ends_with("Constraint")
                && ["Attachment0", "Attachment1"].iter().any(|key| {
                    part_ref(key)
                        .and_then(|a| world.nodes.parent(&a).flatten())
                        .as_deref()
                        == Some(id)
                })
            {
                return true;
            }
            other.class_name.starts_with("Body") && other.parent.as_deref() == Some(id)
        });
        if connected {
            return Ok(json!({"status":"stale_authority","frames_applied":0}));
        }
        staged.push((
            id.to_owned(),
            encode_frame(&decoded),
            json!({"$vector3":linear}),
            json!({"$vector3":angular}),
        ));
    }
    for (id, cf, linear, angular) in &staged {
        let decoded = cframe_from_json(cf).map_err(|e| e.to_string())?;
        for (key, value) in part_fields(&decoded) {
            world::set_engine_property(ctx, id, key, value, Some(revision))
                .map_err(|e| e.to_string())?;
        }
        world::set_engine_property(
            ctx,
            id,
            "AssemblyLinearVelocity",
            linear.clone(),
            Some(revision),
        )
        .map_err(|e| e.to_string())?;
        world::set_engine_property(
            ctx,
            id,
            "AssemblyAngularVelocity",
            angular.clone(),
            Some(revision),
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(json!({"status":"applied","frames_applied":staged.len()}))
}

/// Session-facing owned-part adapter that preserves silent physics observations.
pub fn apply_engine_owned_parts_with_lua(
    lua: &Lua,
    ctx: &WorldRef,
    batch: &Json,
    scope: u32,
) -> mlua::Result<Json> {
    pose_events::apply(lua, ctx, batch, pose_events::BODY_KEYS, batch["source_revision"].as_u64(),
        || apply_engine_owned_parts(ctx, batch, scope))
}

#[path = "engine_pose_events.rs"]
mod pose_events;

#[path = "humanoid_feedback.rs"]
mod feedback;
pub use feedback::apply as apply_physics_feedback;

#[cfg(test)]
#[path = "movement_tests.rs"]
mod tests;
