//! Local, fixture-backed service bindings for the Rust Luau host.
//!
//! These adapters expose values already present in the current World. They do
//! not make Roblox requests or manufacture server/session telemetry.

use super::{
    values,
    world::{self, WorldRef},
};
use mlua::{Lua, MultiValue, ObjectLike, Value};
use serde_json::{json, Value as Json};
use std::{
    cell::RefCell,
    collections::{BTreeSet, HashMap, VecDeque},
    rc::Rc,
    time::{Duration, Instant},
};

const MAX_URL_BYTES: usize = 128 * 1024;
const MAX_TEST_OUTPUT_BYTES: usize = 1024 * 1024;
const MAX_DEBRIS_ITEMS: usize = 1000;

struct ScheduledDebris {
    instance_id: String,
    deadline: Instant,
}

type DebrisQueues = Rc<RefCell<HashMap<usize, VecDeque<ScheduledDebris>>>>;
type PlayerMouseCache = Rc<RefCell<HashMap<(usize, String), String>>>;

fn debris_queues(lua: &Lua) -> DebrisQueues {
    if lua.app_data_ref::<DebrisQueues>().is_none() {
        lua.set_app_data(DebrisQueues::default());
    }
    lua.app_data_ref::<DebrisQueues>()
        .expect("debris queue installed")
        .clone()
}

fn world_key(ctx: &WorldRef) -> usize {
    Rc::as_ptr(ctx) as usize
}

/// Pump delayed local Debris destruction from the host's world tick.
pub fn tick(lua: &Lua, ctx: &WorldRef) -> mlua::Result<()> {
    let Some(queues) = lua.app_data_ref::<DebrisQueues>().map(|queues| queues.clone()) else {
        return Ok(());
    };
    let due = {
        let mut queues = queues.borrow_mut();
        let key = world_key(ctx);
        let Some(queue) = queues.get_mut(&key) else { return Ok(()); };
        let now = Instant::now();
        let mut due = Vec::new();
        queue.retain(|item| {
            if item.deadline <= now {
                due.push(item.instance_id.clone());
                false
            } else {
                true
            }
        });
        if queue.is_empty() {
            queues.remove(&key);
        }
        due
    };
    let mut first_error = None;
    for id in due {
        if let Err(error) = world::destroy_instance(lua, ctx, &id) {
            first_error.get_or_insert(error);
        }
    }
    first_error.map_or(Ok(()), Err)
}

fn collect_args(values: MultiValue) -> Vec<Value> {
    values.into_iter().collect()
}

fn expected(class: &str, method: &str) -> mlua::Error {
    mlua::Error::RuntimeError(format!("{method} requires {class}"))
}

fn tag_arg(value: &Value) -> mlua::Result<String> {
    let Value::String(value) = value else {
        return Err(mlua::Error::RuntimeError("tag must be a string".into()));
    };
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 65_536 || bytes.contains(&0) {
        return Err(mlua::Error::RuntimeError(
            "tag must be non-empty UTF-8 without NUL and at most 65536 bytes".into(),
        ));
    }
    Ok(value.to_str()?.to_string())
}

fn is_in_datamodel(ctx: &WorldRef, id: &str) -> bool {
    let world = ctx.borrow();
    let root = world
        .nodes
        .first_live_of_class("DataModel")
        .map(|node| node.id.clone());
    let Some(root) = root else { return false };
    let mut current = Some(id.to_owned());
    let mut remaining = world.nodes.len().saturating_add(1);
    while let Some(node_id) = current {
        if node_id == root {
            return true;
        }
        if remaining == 0 {
            return false;
        }
        remaining -= 1;
        current = world
            .nodes
            .get(&node_id)
            .and_then(|node| node.parent.clone());
    }
    false
}

fn read_tags(ctx: &WorldRef, id: &str) -> mlua::Result<Vec<String>> {
    let world = ctx.borrow();
    let node = world
        .nodes
        .get(id)
        .ok_or_else(|| mlua::Error::RuntimeError("tag target is not in this world".into()))?;
    if node.destroyed {
        return Err(mlua::Error::RuntimeError("tag target is destroyed".into()));
    }
    if !node.collection_tags_known {
        return Err(mlua::Error::RuntimeError(
            "CollectionService tag state is unknown for this instance".into(),
        ));
    }
    Ok(node.collection_tags.clone())
}

fn fire_tag(lua: &Lua, ctx: &WorldRef, id: &str, tag: &str, added: bool) -> mlua::Result<()> {
    let service = ctx.borrow().services.get("CollectionService").cloned();
    if let Some(service) = service {
        let args = MultiValue::from_vec(vec![world::instance(lua, ctx, id)?]);
        super::signals::fire(
            lua,
            ctx,
            &service,
            &format!("tag-{}:{tag}", if added { "added" } else { "removed" }),
            args,
        )?;
    }
    Ok(())
}

fn change_tag(lua: &Lua, ctx: &WorldRef, id: &str, tag: &str, add: bool) -> mlua::Result<()> {
    let mut tags = read_tags(ctx, id)?;
    let already_present = tags.iter().any(|existing| existing == tag);
    if already_present == add {
        return Ok(());
    }
    if add {
        if tags.len() >= 256 {
            return Err(mlua::Error::RuntimeError(
                "instance tag limit exceeded".into(),
            ));
        }
        tags.push(tag.to_owned());
        tags.sort();
    } else {
        tags.retain(|existing| existing != tag);
    }
    let actor_internal_revision = world::uses_actor_namespace(ctx);
    let base_local_revision = world::bump_local_revision(ctx, id)?;
    {
        let mut world = ctx.borrow_mut();
        let node = world
            .nodes
            .get_mut(id)
            .ok_or_else(|| mlua::Error::RuntimeError("tag target is not in this world".into()))?;
        node.collection_tags = tags.clone();
        node.collection_tags_known = true;
        let mut mutation = json!({"kind":"set_tags","id":id,"tags":tags});
        if actor_internal_revision {
            mutation["_base_local_revision"] = json!(base_local_revision);
            mutation["_result_local_revision"] = json!(node.local_revision);
        }
        world
            .effects
            .entry("mutations")
            .or_insert_with(|| Json::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| mlua::Error::RuntimeError("mutation effect list is invalid".into()))?
            .push(mutation);
    }
    if is_in_datamodel(ctx, id) {
        fire_tag(lua, ctx, id, tag, add)?;
    }
    Ok(())
}

fn collection_method(
    lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    method: &str,
    args: MultiValue,
) -> mlua::Result<MultiValue> {
    let values = collect_args(args);
    if ctx.borrow().nodes.get(id).map(|n| n.class_name.as_str()) != Some("CollectionService") {
        return Err(expected("CollectionService", method));
    }
    let mut out = MultiValue::new();
    match method {
        "AddTag" | "RemoveTag" | "HasTag" | "GetTags" => {
            let Some(target_value) = values.first() else {
                return Err(mlua::Error::RuntimeError(
                    "expected tag target Instance".into(),
                ));
            };
            if !world::instance_belongs_to(target_value, ctx) {
                return Err(mlua::Error::RuntimeError(
                    "tag target belongs to a different world".into(),
                ));
            }
            let target = world::instance_id(target_value)
                .ok_or_else(|| mlua::Error::RuntimeError("expected tag target Instance".into()))?;
            if !ctx.borrow().nodes.contains_key(&target) {
                return Err(mlua::Error::RuntimeError(
                    "tag target belongs to a different world".into(),
                ));
            }
            match method {
                "GetTags" => {
                    if values.len() != 1 {
                        return Err(mlua::Error::RuntimeError(
                            "GetTags expects one Instance".into(),
                        ));
                    }
                    let tags = read_tags(ctx, &target)?;
                    let table = lua.create_table_with_capacity(tags.len(), 0)?;
                    for (i, tag) in tags.iter().enumerate() {
                        table.raw_set(i + 1, tag.as_str())?;
                    }
                    out.push_back(Value::Table(table));
                }
                _ => {
                    if values.len() != 2 {
                        return Err(mlua::Error::RuntimeError(format!(
                            "{method} expects an Instance and tag"
                        )));
                    }
                    let tag = tag_arg(&values[1])?;
                    match method {
                        "AddTag" => change_tag(lua, ctx, &target, &tag, true)?,
                        "RemoveTag" => change_tag(lua, ctx, &target, &tag, false)?,
                        "HasTag" => out.push_back(Value::Boolean(
                            read_tags(ctx, &target)?.iter().any(|t| t == &tag),
                        )),
                        _ => unreachable!(),
                    }
                }
            }
        }
        "GetTagged" => {
            if values.len() != 1 {
                return Err(mlua::Error::RuntimeError("GetTagged expects a tag".into()));
            }
            let tag = tag_arg(&values[0])?;
            let candidates: Vec<String> = {
                let world = ctx.borrow();
                world
                    .nodes
                    .materialized_values()
                    .filter(|n| {
                        !n.destroyed
                            && n.collection_tags_known
                            && n.collection_tags.iter().any(|t| t == &tag)
                    })
                    .map(|n| n.id.clone())
                    .collect()
            };
            let tagged: Vec<String> = candidates
                .into_iter()
                .filter(|node_id| is_in_datamodel(ctx, node_id))
                .collect();
            let table = lua.create_table_with_capacity(tagged.len(), 0)?;
            for (i, node) in tagged.iter().enumerate() {
                table.raw_set(i + 1, world::instance(lua, ctx, node)?)?;
            }
            out.push_back(Value::Table(table));
        }
        "GetAllTags" => {
            if !values.is_empty() {
                return Err(mlua::Error::RuntimeError(
                    "GetAllTags takes no arguments".into(),
                ));
            }
            let candidates: Vec<(String, Vec<String>)> = ctx
                .borrow()
                .nodes
                .materialized_values()
                .filter(|n| !n.destroyed && n.collection_tags_known)
                .map(|n| (n.id.clone(), n.collection_tags.clone()))
                .collect();
            let tags: BTreeSet<String> = candidates
                .into_iter()
                .filter(|(id, _)| is_in_datamodel(ctx, id))
                .flat_map(|(_, tags)| tags)
                .collect();
            let table = lua.create_table_with_capacity(tags.len(), 0)?;
            for (i, tag) in tags.iter().enumerate() {
                table.raw_set(i + 1, tag.as_str())?;
            }
            out.push_back(Value::Table(table));
        }
        "GetInstanceAddedSignal" | "GetInstanceRemovedSignal" => {
            if values.len() != 1 {
                return Err(mlua::Error::RuntimeError(format!(
                    "{method} expects one tag"
                )));
            }
            let tag = tag_arg(&values[0])?;
            let event = format!(
                "tag-{}:{tag}",
                if method == "GetInstanceAddedSignal" {
                    "added"
                } else {
                    "removed"
                }
            );
            out.push_back(super::signals::signal(lua, ctx, id, &event)?);
        }
        _ => return Err(expected("CollectionService", method)),
    }
    Ok(out)
}

fn instance_tag_method(
    lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    method: &str,
    args: MultiValue,
) -> mlua::Result<MultiValue> {
    let values = collect_args(args);
    let mut out = MultiValue::new();
    match method {
        "GetTags" => {
            if !values.is_empty() {
                return Err(mlua::Error::RuntimeError(
                    "GetTags takes no arguments".into(),
                ));
            }
            let tags = read_tags(ctx, id)?;
            let table = lua.create_table_with_capacity(tags.len(), 0)?;
            for (i, tag) in tags.iter().enumerate() {
                table.raw_set(i + 1, tag.as_str())?;
            }
            out.push_back(Value::Table(table));
        }
        "AddTag" | "RemoveTag" | "HasTag" => {
            if values.len() != 1 {
                return Err(mlua::Error::RuntimeError(format!(
                    "{method} expects one tag"
                )));
            }
            let tag = tag_arg(&values[0])?;
            match method {
                "AddTag" => change_tag(lua, ctx, id, &tag, true)?,
                "RemoveTag" => change_tag(lua, ctx, id, &tag, false)?,
                "HasTag" => out.push_back(Value::Boolean(
                    read_tags(ctx, id)?.iter().any(|t| t == &tag),
                )),
                _ => unreachable!(),
            }
        }
        _ => return Err(expected("Instance", method)),
    }
    Ok(out)
}

fn debris_add_item(
    lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    args: MultiValue,
) -> mlua::Result<MultiValue> {
    if ctx.borrow().nodes.get(id).map(|n| n.class_name.as_str()) != Some("Debris") {
        return Err(expected("Debris", "AddItem"));
    }
    let values = collect_args(args);
    let Some(item) = values.first() else {
        return Err(mlua::Error::RuntimeError(
            "Debris:AddItem expects an Instance".into(),
        ));
    };
    if !world::instance_belongs_to(item, ctx) {
        return Err(mlua::Error::RuntimeError(
            "item belongs to a different world".into(),
        ));
    }
    let item_id = world::instance_id(item)
        .ok_or_else(|| mlua::Error::RuntimeError("Debris:AddItem expects an Instance".into()))?;
    if !ctx.borrow().nodes.contains_key(&item_id) {
        return Err(mlua::Error::RuntimeError(
            "item belongs to a different world".into(),
        ));
    }
    let seconds = match values.get(1) {
        None | Some(Value::Nil) => 10.0,
        Some(Value::Integer(value)) => *value as f64,
        Some(Value::Number(value)) => *value,
        _ => {
            return Err(mlua::Error::RuntimeError(
                "lifetime must be a number".into(),
            ))
        }
    };
    if !seconds.is_finite() {
        return Err(mlua::Error::RuntimeError("lifetime must be finite".into()));
    }
    if values.len() > 2 {
        return Err(mlua::Error::RuntimeError(
            "Debris:AddItem accepts at most two arguments".into(),
        ));
    }
    if ctx
        .borrow()
        .nodes
        .get(&item_id)
        .is_some_and(|n| n.destroyed)
    {
        return Ok(MultiValue::new());
    }
    let queues = debris_queues(lua);
    // Very large finite lifetimes are bounded to avoid Duration overflow.
    let delay = Duration::from_secs_f64(seconds.clamp(0.0, 1.0e9));
    let evicted = {
        let mut queues = queues.borrow_mut();
        let queue = queues.entry(world_key(ctx)).or_default();
        let evicted = if queue.len() >= MAX_DEBRIS_ITEMS { queue.pop_front() } else { None };
        queue.push_back(ScheduledDebris {
            instance_id: item_id,
            deadline: Instant::now() + delay,
        });
        evicted
    };
    if let Some(oldest) = evicted {
        world::destroy_instance(lua, ctx, &oldest.instance_id)?;
    }
    Ok(MultiValue::new())
}

fn stats_item_method(
    lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    method: &str,
    args: MultiValue,
) -> mlua::Result<MultiValue> {
    if !args.is_empty() {
        return Err(mlua::Error::RuntimeError(format!(
            "{method} takes no arguments"
        )));
    }
    let node = ctx
        .borrow()
        .nodes
        .get(id)
        .cloned()
        .ok_or_else(|| mlua::Error::RuntimeError("StatsItem is unavailable".into()))?;
    if node.class_name != "StatsItem" {
        return Err(expected("StatsItem", method));
    }
    let measured_rtt = if node.name == "Data Ping" {
        ctx.borrow()
            .snapshot
            .get("session_stats")
            .and_then(|stats| stats.get("network_rtt_ms"))
            .and_then(Json::as_f64)
            .filter(|v| v.is_finite() && *v >= 0.0)
    } else {
        None
    };
    let mut out = MultiValue::new();
    match method {
        "GetValue" => {
            if let Some(value) = measured_rtt {
                out.push_back(Value::Number(value));
            } else {
                out.push_back(values::from_json(
                    lua,
                    &world::get_property(ctx, id, "Value")?,
                )?);
            }
        }
        "GetValueString" => {
            let value = if let Some(value) = measured_rtt {
                format!("{value:.3} ms")
            } else {
                world::get_property(ctx, id, "ValueString")?
                    .as_str()
                    .map(ToOwned::to_owned)
                    .ok_or_else(|| {
                        mlua::Error::RuntimeError(
                            "StatsItem value string is unavailable in snapshot".into(),
                        )
                    })?
            };
            out.push_back(Value::String(lua.create_string(value)?));
        }
        _ => return Err(expected("StatsItem", method)),
    }
    Ok(out)
}

fn stats_network(lua: &Lua, ctx: &WorldRef, id: &str) -> mlua::Result<Option<Value>> {
    if ctx.borrow().nodes.get(id).map(|n| n.class_name.as_str()) != Some("Stats") {
        return Ok(None);
    }
    fn item(lua: &Lua, ctx: &WorldRef, parent: &str, name: &str) -> mlua::Result<String> {
        if let Some(child) = world::children(ctx, parent).into_iter().find(|child| {
            ctx.borrow().nodes.get(child).is_some_and(|n| n.name == name)
        }) { return Ok(child); }
        // Engine-owned StatsItems are not script-creatable. No default numeric
        // value is supplied: GetValue still requires an actual measurement.
        let value = world::create_internal_node(lua, ctx, "StatsItem", Some(parent), Default::default())?;
        let reference = values::to_json(lua, &value)?;
        let id = reference["$ref"].as_str().ok_or_else(|| expected("StatsItem", "Network"))?;
        world::set_property(lua, ctx, id, "Name", json!(name))?;
        Ok(id.to_owned())
    }
    let network = item(lua, ctx, id, "Network")?;
    let server = item(lua, ctx, &network, "ServerStatsItem")?;
    item(lua, ctx, &server, "Data Ping")?;
    Ok(Some(world::instance(lua, ctx, &network)?))
}

fn text_service_get_text_size(
    lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    args: MultiValue,
) -> mlua::Result<MultiValue> {
    if ctx.borrow().nodes.get(id).map(|n| n.class_name.as_str()) != Some("TextService") {
        return Err(expected("TextService", "GetTextSize"));
    }
    let args = collect_args(args);
    if args.len() != 4 {
        return Err(mlua::Error::RuntimeError(
            "GetTextSize expects text, fontSize, font and frameSize".into(),
        ));
    }
    let Value::String(text) = &args[0] else {
        return Err(mlua::Error::RuntimeError("text must be a string".into()));
    };
    let text = text.to_str()?;
    if text.as_bytes().len() > 4096 || text.as_bytes().contains(&0) {
        return Err(mlua::Error::RuntimeError(
            "text must be valid UTF-8 up to 4096 bytes without NUL".into(),
        ));
    }
    let size = match &args[1] {
        Value::Integer(value) => *value as f64,
        Value::Number(value) => *value,
        _ => {
            return Err(mlua::Error::RuntimeError(
                "fontSize must be a number".into(),
            ))
        }
    };
    if !size.is_finite() || !(1.0..=1000.0).contains(&size) || size.fract() != 0.0 {
        return Err(mlua::Error::RuntimeError(
            "fontSize must be an integer in 1..1000".into(),
        ));
    }
    let Value::UserData(frame) = &args[3] else {
        return Err(mlua::Error::RuntimeError(
            "frameSize must be Vector2".into(),
        ));
    };
    let frame = frame
        .borrow::<super::numeric::Vector2>()
        .map_err(|_| mlua::Error::RuntimeError("frameSize must be Vector2".into()))?;
    if !frame.x.is_finite()
        || !frame.y.is_finite()
        || !(0.0..=32768.0).contains(&frame.x)
        || !(0.0..=32768.0).contains(&frame.y)
    {
        return Err(mlua::Error::RuntimeError(
            "frameSize must contain finite values in 0..32768".into(),
        ));
    }
    let font_table: mlua::Table = lua.globals().get("Font")?;
    let from_enum: mlua::Function = font_table.get("fromEnum")?;
    let font_value: Value = from_enum.call(args[2].clone())?;
    let font = values::to_json(lua, &font_value)?;
    let request = json!({
        "request_id":1,"kind":"bounds","text":text.as_ref(),"size":size,
        "width":frame.x,"font":font.get("$font").cloned().ok_or_else(|| mlua::Error::RuntimeError("invalid font descriptor".into()))?
    });
    let raw = serde_json::to_vec(&request).map_err(mlua::Error::external)?;
    let bounds = crate::engine_text_measure::measure(&raw).map_err(mlua::Error::RuntimeError)?;
    let vector = super::numeric::push_vector2(lua, bounds[0], bounds[1])?;
    Ok(MultiValue::from_vec(vec![Value::UserData(vector)]))
}

fn players(
    lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    method: &str,
    args: MultiValue,
) -> mlua::Result<MultiValue> {
    let values = collect_args(args);
    let list = {
        let world = ctx.borrow();
        let node = world
            .nodes
            .get(id)
            .ok_or_else(|| mlua::Error::RuntimeError("instance is not in this world".into()))?;
        if node.class_name != "Players" {
            return Err(expected("Players", method));
        }
        world::children(ctx, id)
            .into_iter()
            .filter(|child| {
                world
                    .nodes
                    .get(child)
                    .is_some_and(|n| n.class_name == "Player")
            })
            .collect::<Vec<_>>()
    };
    let mut result = MultiValue::new();
    match method {
        "GetPlayers" | "getPlayers" | "players" => {
            if !values.is_empty() {
                return Err(mlua::Error::RuntimeError(
                    "GetPlayers takes no arguments".into(),
                ));
            }
            let table = lua.create_table_with_capacity(list.len(), 0)?;
            for (index, player) in list.iter().enumerate() {
                table.raw_set(index + 1, world::instance(lua, ctx, player)?)?;
            }
            result.push_back(Value::Table(table));
        }
        "GetPlayerByUserId" => {
            let requested = match values.first() {
                Some(Value::Integer(value)) => *value as f64,
                Some(Value::Number(value)) if value.is_finite() && value.fract() == 0.0 => *value,
                _ => {
                    return Err(mlua::Error::RuntimeError(
                        "GetPlayerByUserId expects an integer UserId".into(),
                    ))
                }
            };
            if values.len() != 1 {
                return Err(mlua::Error::RuntimeError(
                    "GetPlayerByUserId expects one argument".into(),
                ));
            }
            let found = list.into_iter().find(|player| {
                world::get_property(ctx, player, "UserId")
                    .ok()
                    .and_then(|value| value.as_f64())
                    .is_some_and(|v| v == requested)
            });
            result.push_back(match found {
                Some(player) => world::instance(lua, ctx, &player)?,
                None => Value::Nil,
            });
        }
        "GetPlayerFromCharacter" => {
            let Some(character_value) = values.first() else {
                return Err(mlua::Error::RuntimeError(
                    "GetPlayerFromCharacter expects an Instance".into(),
                ));
            };
            if !world::instance_belongs_to(character_value, ctx) {
                return Err(mlua::Error::RuntimeError(
                    "character belongs to a different world".into(),
                ));
            }
            let character = world::instance_id(character_value).ok_or_else(|| {
                mlua::Error::RuntimeError("GetPlayerFromCharacter expects an Instance".into())
            })?;
            if values.len() != 1 {
                return Err(mlua::Error::RuntimeError(
                    "GetPlayerFromCharacter expects one argument".into(),
                ));
            }
            if !ctx.borrow().nodes.contains_key(&character) {
                return Err(mlua::Error::RuntimeError(
                    "character belongs to a different world".into(),
                ));
            }
            if !world::is_a(ctx, &character, "Model") {
                result.push_back(Value::Nil);
                return Ok(result);
            }
            let found = list.into_iter().find(|player| {
                world::get_property(ctx, player, "Character")
                    .ok()
                    .and_then(|v| v.get("$ref").and_then(Json::as_str).map(ToOwned::to_owned))
                    .as_deref()
                    == Some(character.as_str())
            });
            result.push_back(match found {
                Some(player) => world::instance(lua, ctx, &player)?,
                None => Value::Nil,
            });
        }
        _ => return Err(expected("Players", method)),
    }
    Ok(result)
}

fn player_method(
    lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    method: &str,
    args: MultiValue,
) -> mlua::Result<MultiValue> {
    let values = collect_args(args);
    let node = ctx
        .borrow()
        .nodes
        .get(id)
        .cloned()
        .ok_or_else(|| mlua::Error::RuntimeError("instance is not in this world".into()))?;
    if node.class_name != "Player" {
        return Err(expected("Player", method));
    }
    let mut result = MultiValue::new();
    match method {
        "DistanceFromCharacter" => {
            if values.len() != 1 {
                return Err(mlua::Error::RuntimeError(
                    "DistanceFromCharacter expects one Vector3".into(),
                ));
            }
            let Some(Value::Vector(point)) = values.first() else {
                return Err(mlua::Error::RuntimeError(
                    "DistanceFromCharacter expects Vector3".into(),
                ));
            };
            let character = world::get_property(ctx, id, "Character")?;
            let Some(character_id) = character.get("$ref").and_then(Json::as_str) else {
                if character.is_null() {
                    result.push_back(Value::Number(0.0));
                    return Ok(result);
                }
                return Err(mlua::Error::RuntimeError(
                    "Character is unavailable in snapshot".into(),
                ));
            };
            let mut root = world::children(ctx, character_id)
                .into_iter()
                .find(|child| {
                    ctx.borrow()
                        .nodes
                        .get(child)
                        .is_some_and(|n| n.name == "HumanoidRootPart")
                });
            if root.is_none() {
                root = world::get_property(ctx, character_id, "PrimaryPart")
                    .ok()
                    .and_then(|value| {
                        value
                            .get("$ref")
                            .and_then(Json::as_str)
                            .map(ToOwned::to_owned)
                    });
            }
            let Some(root) = root else {
                result.push_back(Value::Number(0.0));
                return Ok(result);
            };
            if !world::is_a(ctx, &root, "BasePart") {
                result.push_back(Value::Number(0.0));
                return Ok(result);
            }
            let position = world::get_property(ctx, &root, "Position")?;
            let coords = position
                .get("$vector3")
                .and_then(Json::as_array)
                .filter(|v| v.len() == 3)
                .ok_or_else(|| {
                    mlua::Error::RuntimeError("HumanoidRootPart.Position is unavailable".into())
                })?;
            let xyz = [
                coords[0]
                    .as_f64()
                    .ok_or_else(|| mlua::Error::RuntimeError("position x is unavailable".into()))?,
                coords[1]
                    .as_f64()
                    .ok_or_else(|| mlua::Error::RuntimeError("position y is unavailable".into()))?,
                coords[2]
                    .as_f64()
                    .ok_or_else(|| mlua::Error::RuntimeError("position z is unavailable".into()))?,
            ];
            let delta = [
                f64::from(point.x()) - xyz[0],
                f64::from(point.y()) - xyz[1],
                f64::from(point.z()) - xyz[2],
            ];
            result.push_back(Value::Number(
                (delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]).sqrt(),
            ));
        }
        "GetMouse" => {
            if !values.is_empty() {
                return Err(mlua::Error::RuntimeError(
                    "GetMouse takes no arguments".into(),
                ));
            }
            if local_player_id(ctx).as_deref() != Some(id) {
                return Err(mlua::Error::RuntimeError(
                    "GetMouse requires LocalPlayer".into(),
                ));
            }
            let mouse_id = local_mouse(lua, ctx, id)?;
            result.push_back(world::instance(lua, ctx, &mouse_id)?);
        }
        _ => return Err(expected("Player", method)),
    }
    Ok(result)
}

/// Stable per-VM PlayerMouse for the LocalPlayer, creating it on first use.
/// Shared by `Player:GetMouse()` and the VirtualUser mouse surface so both see
/// the same instance and coordinate/hit state.
pub(crate) fn local_mouse(lua: &Lua, ctx: &WorldRef, player: &str) -> mlua::Result<String> {
    if lua.app_data_ref::<PlayerMouseCache>().is_none() {
        lua.set_app_data(PlayerMouseCache::default());
    }
    let cache = lua
        .app_data_ref::<PlayerMouseCache>()
        .expect("mouse cache installed")
        .clone();
    let key = (world_key(ctx), player.to_owned());
    let cached = cache.borrow().get(&key).cloned();
    if let Some(cached) =
        cached.filter(|mouse| ctx.borrow().nodes.get(mouse).is_some_and(|n| !n.destroyed))
    {
        return Ok(cached);
    }
    let properties = serde_json::Map::from_iter([
        ("Icon".to_owned(), json!("")),
        ("TargetFilter".to_owned(), Json::Null),
    ]);
    let mouse = world::create_internal_node(lua, ctx, "PlayerMouse", None, properties)?;
    let mouse_id = world::instance_id(&mouse)
        .ok_or_else(|| mlua::Error::RuntimeError("could not create PlayerMouse".into()))?;
    if let Value::UserData(userdata) = mouse {
        userdata.set("Name", "Mouse")?;
        userdata.set("Icon", "")?;
        userdata.set("TargetFilter", Value::Nil)?;
    }
    cache.borrow_mut().insert(key, mouse_id.clone());
    Ok(mouse_id)
}

/// `Players:GetUserThumbnailAsync` returns `(content, isReady)`. There is no
/// CDN fetch here, so report a stable constructed URL and mark it ready.
fn players_thumbnail(lua: &Lua, ctx: &WorldRef, id: &str, args: MultiValue) -> mlua::Result<MultiValue> {
    if ctx.borrow().nodes.get(id).map(|n| n.class_name.as_str()) != Some("Players") {
        return Err(expected("Players", "GetUserThumbnailAsync"));
    }
    let values = collect_args(args);
    let user_id = values
        .first()
        .and_then(|v| match v {
            Value::Integer(n) => Some(*n),
            Value::Number(n) if n.fract() == 0. => Some(*n as i64),
            _ => None,
        })
        .ok_or_else(|| {
            mlua::Error::RuntimeError("GetUserThumbnailAsync expects a UserId integer".into())
        })?;
    let size = match values.get(2) {
        Some(Value::UserData(item)) => {
            super::enums::enum_item_name(item).unwrap_or_else(|_| "Size420x420".into())
        }
        _ => "Size420x420".to_owned(),
    };
    let dimensions = match size.as_str() {
        "Size150x150" => "150x150",
        "Size180x180" => "180x180",
        "Size352x352" => "352x352",
        _ => "420x420",
    };
    let url = format!(
        "https://www.roblox.com/headshot-thumbnail/image?userId={user_id}&width={dimensions}&height={dimensions}&format=png"
    );
    Ok(MultiValue::from_vec(vec![
        Value::String(lua.create_string(&url)?),
        Value::Boolean(true),
    ]))
}

/// LocalizationService + Translator. Without downloaded translations every
/// Translate/FormatByKey passes the source text straight through, which is
/// what the engine does when no localization table matches.
fn localization_method(lua: &Lua, ctx: &WorldRef, id: &str, kind: &str, args: MultiValue) -> mlua::Result<MultiValue> {
    let class = ctx
        .borrow()
        .nodes
        .get(id)
        .map(|n| n.class_name.clone())
        .unwrap_or_default();
    let values = collect_args(args);
    let mut out = MultiValue::new();
    match (class.as_str(), kind) {
        ("LocalizationService", "GetTranslatorForPlayer" | "GetTranslatorForPlayerAsync") => {
            let translator = world::create_internal_node(
                lua,
                ctx,
                "Translator",
                None,
                serde_json::Map::from_iter([(
                    "LocaleId".to_owned(),
                    json!("en-us"),
                )]),
            )?;
            out.push_back(translator);
        }
        ("LocalizationService", "GetCorescriptLocalizations") => {
            out.push_back(Value::Table(lua.create_table()?));
        }
        ("LocalizationService", "GetTableEntries") => {
            out.push_back(Value::Table(lua.create_table()?));
        }
        ("LocalizationService", "GetIsLoadingInternalTranslations") => {
            out.push_back(Value::Boolean(false));
        }
        ("LocalizationService", "SetRobloxLocaleId" | "StartTextScraper") => {}
        ("Translator", "Translate" | "FormatByKey") => {
            if kind == "Translate" {
                out.push_back(values.first().cloned().unwrap_or(Value::Nil));
            } else {
                // FormatByKey(key, args) returns the key when no table matches.
                out.push_back(values.first().cloned().unwrap_or(Value::Nil));
            }
            out.push_back(values.first().cloned().unwrap_or(Value::Nil));
        }
        ("Translator", "RobloxOnlyTranslate") => {
            out.push_back(Value::Boolean(false));
        }
        _ => return Err(expected(&class, kind)),
    }
    Ok(out)
}

macro_rules! localization_wrapper {
    ($name:ident, $kind:literal) => {
        fn $name(lua: &Lua, ctx: &WorldRef, id: &str, args: MultiValue) -> mlua::Result<MultiValue> {
            localization_method(lua, ctx, id, $kind, args)
        }
    };
}
localization_wrapper!(loc_translator, "GetTranslatorForPlayer");
localization_wrapper!(loc_translator_async, "GetTranslatorForPlayerAsync");
localization_wrapper!(translator_translate, "Translate");
localization_wrapper!(translator_format, "FormatByKey");

fn http_method(
    lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    method: &str,
    args: MultiValue,
) -> mlua::Result<MultiValue> {
    let values = collect_args(args);
    if ctx.borrow().nodes.get(id).map(|n| n.class_name.as_str()) != Some("HttpService") {
        return Err(expected("HttpService", method));
    }
    let mut result = MultiValue::new();
    match method {
        "JSONEncode" => {
            if values.len() != 1 {
                return Err(mlua::Error::RuntimeError(
                    "JSONEncode expects one value".into(),
                ));
            }
            let encoded = values::json_encode_value(lua, &values[0])?;
            result.push_back(Value::String(lua.create_string(
                serde_json::to_string(&encoded).map_err(mlua::Error::external)?,
            )?));
        }
        "JSONDecode" => {
            if values.len() != 1 {
                return Err(mlua::Error::RuntimeError(
                    "JSONDecode expects one string".into(),
                ));
            }
            let Value::String(input) = &values[0] else {
                return Err(mlua::Error::RuntimeError(
                    "JSONDecode expects a string".into(),
                ));
            };
            let decoded: Json =
                serde_json::from_slice(&input.as_bytes()).map_err(mlua::Error::external)?;
            result.push_back(values::from_json(lua, &decoded)?);
        }
        "UrlEncode" => {
            if values.len() != 1 {
                return Err(mlua::Error::RuntimeError(
                    "UrlEncode expects one string".into(),
                ));
            }
            let Value::String(input) = &values[0] else {
                return Err(mlua::Error::RuntimeError(
                    "UrlEncode expects a string".into(),
                ));
            };
            let bytes = input.as_bytes();
            if bytes.len() > MAX_URL_BYTES {
                return Err(mlua::Error::RuntimeError("input exceeds 128 KiB".into()));
            }
            let mut output = Vec::with_capacity(bytes.len() * 3);
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            for &byte in bytes.iter() {
                if byte.is_ascii_alphanumeric() {
                    output.push(byte);
                } else {
                    output.extend_from_slice(&[
                        b'%',
                        HEX[(byte >> 4) as usize],
                        HEX[(byte & 0x0f) as usize],
                    ]);
                }
            }
            result.push_back(Value::String(lua.create_string(output)?));
        }
        _ => return Err(expected("HttpService", method)),
    }
    Ok(result)
}

fn test_service(
    _lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    method: &str,
    args: MultiValue,
) -> mlua::Result<MultiValue> {
    if ctx.borrow().nodes.get(id).map(|n| n.class_name.as_str()) != Some("TestService") {
        return Err(expected("TestService", method));
    }
    if method != "Message" {
        return Err(expected("TestService", method));
    }
    let values = collect_args(args);
    let Some(Value::String(message)) = values.first() else {
        return Err(mlua::Error::RuntimeError(
            "TestService:Message expects a string".into(),
        ));
    };
    if values.len() > 3 {
        return Err(mlua::Error::RuntimeError(
            "TestService:Message accepts at most three arguments".into(),
        ));
    }
    if let Some(source) = values.get(1).filter(|v| !matches!(v, Value::Nil)) {
        if !world::instance_belongs_to(source, ctx) {
            return Err(mlua::Error::RuntimeError(
                "source belongs to a different world".into(),
            ));
        }
        let Some(source_id) = world::instance_id(source) else {
            return Err(mlua::Error::RuntimeError(
                "source must be an Instance".into(),
            ));
        };
        if !ctx.borrow().nodes.contains_key(&source_id) {
            return Err(mlua::Error::RuntimeError(
                "source belongs to a different world".into(),
            ));
        }
    }
    let seconds = match values.get(2) {
        None | Some(Value::Nil) => None,
        Some(Value::Number(value)) if value.is_finite() => Some(*value),
        Some(Value::Integer(value)) => Some(*value as f64),
        _ => {
            return Err(mlua::Error::RuntimeError(
                "timestamp must be a finite number".into(),
            ))
        }
    };
    let text = message.to_str()?.to_string();
    let rendered = format!("TestService: {text}\n");
    let mut world = ctx.borrow_mut();
    let events = world
        .effects
        .entry("test_output")
        .or_insert_with(|| Json::Array(Vec::new()));
    if let Json::Array(events) = events {
        let used: usize = events
            .iter()
            .filter_map(|event| event.get("text").and_then(Json::as_str))
            .map(str::len)
            .sum();
        if used < MAX_TEST_OUTPUT_BYTES {
            let remaining = MAX_TEST_OUTPUT_BYTES - used;
            let safe_end = rendered
                .char_indices()
                .take_while(|(index, _)| *index <= remaining)
                .map(|(index, _)| index)
                .last()
                .unwrap_or(0);
            let rendered = if rendered.len() > remaining {
                &rendered[..safe_end]
            } else {
                &rendered
            };
            events.push(json!({"kind":"message","text":rendered,"source":values.get(1).and_then(world::instance_id),"timestamp":seconds}));
        }
    }
    Ok(MultiValue::new())
}

fn run_state(
    _lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    method: &str,
    args: MultiValue,
) -> mlua::Result<MultiValue> {
    if ctx.borrow().nodes.get(id).map(|n| n.class_name.as_str()) != Some("RunService") {
        return Err(expected("RunService", method));
    }
    if !args.is_empty() {
        return Err(mlua::Error::RuntimeError(format!(
            "{method} takes no arguments"
        )));
    }
    let name = match method {
        "IsClient" => "is_client",
        "IsServer" => "is_server",
        "IsStudio" => "is_studio",
        "IsEdit" => "is_edit",
        "IsRunMode" => "is_run_mode",
        "IsRunning" => "is_running",
        _ => return Err(expected("RunService", method)),
    };
    // This is the local client's own mode, never a claim about remote server
    // integrity or hidden game state.
    let value = match name {
        "is_client" | "is_running" => true,
        _ => false,
    };
    let mut out = MultiValue::new();
    out.push_back(Value::Boolean(value));
    Ok(out)
}


/// RunService helpers that describe the local client, not remote state.
fn run_info(lua: &Lua, ctx: &WorldRef, id: &str, method: &str) -> mlua::Result<MultiValue> {
    if ctx.borrow().nodes.get(id).map(|n| n.class_name.as_str()) != Some("RunService") {
        return Err(expected("RunService", method));
    }
    let mut out = MultiValue::new();
    match method {
        // The local simulation always steps against the latest authoritative
        // world, so prediction status reports the authoritative tick.
        "GetPredictionStatus" => out.push_back(Value::UserData(super::enums::enum_item(
            lua,
            "PredictionStatus",
            "Authoritative",
        )?)),
        "GetPhysicsStepId" => out.push_back(Value::Integer(
            ctx.borrow().sequence.min(i64::MAX as u64) as i64,
        )),
        "GetRobloxVersion" => {
            let version = ctx
                .borrow()
                .snapshot
                .get("runtime_version")
                .and_then(Json::as_str)
                .unwrap_or("0.745.0.7450000")
                .to_owned();
            out.push_back(Value::String(lua.create_string(version)?));
        }
        "GetCoreScriptVersion" => {
            out.push_back(Value::String(lua.create_string("745")?));
        }
        "GetRobloxClientChannel" => {
            out.push_back(Value::String(lua.create_string("production")?));
        }
        "GetRobloxGuiFocused" => out.push_back(Value::Boolean(false)),
        "getThrottleFramerateEnabled" => out.push_back(Value::Boolean(false)),
        "IsResimulating" | "IsTeamTest" | "IsTeamCreateViewer" => {
            out.push_back(Value::Boolean(false))
        }
        "GetControlAndVariantRolloutFlags"
        | "GetRobloxClientIxpEnrolledExperiments" => {
            out.push_back(Value::Integer(0))
        }
        "GetTotalScriptPlusExecutionTime" => out.push_back(Value::Number(0.)),
        "Set3dRenderingEnabled" | "SetPredictionMode" | "SetRobloxGuiFocused"
        | "setThrottleFramerateEnabled" | "Pause" | "Run" | "Stop" | "Reset" => {}
        _ => return Err(expected("RunService", method)),
    }
    Ok(out)
}
macro_rules! run_info_wrapper {
    ($name:ident, $method:literal) => {
        fn $name(lua: &Lua, ctx: &WorldRef, id: &str, args: MultiValue) -> mlua::Result<MultiValue> {
            let _ = args;
            run_info(lua, ctx, id, $method)
        }
    };
}
run_info_wrapper!(run_prediction_status, "GetPredictionStatus");
run_info_wrapper!(run_physics_step, "GetPhysicsStepId");
run_info_wrapper!(run_version, "GetRobloxVersion");
run_info_wrapper!(run_core_version, "GetCoreScriptVersion");
run_info_wrapper!(run_channel, "GetRobloxClientChannel");
run_info_wrapper!(run_gui_focused, "GetRobloxGuiFocused");
run_info_wrapper!(run_throttle, "getThrottleFramerateEnabled");
run_info_wrapper!(run_resimulating, "IsResimulating");
run_info_wrapper!(run_team_test, "IsTeamTest");
run_info_wrapper!(run_team_viewer, "IsTeamCreateViewer");
run_info_wrapper!(run_rollout, "GetControlAndVariantRolloutFlags");
run_info_wrapper!(run_ixp, "GetRobloxClientIxpEnrolledExperiments");
run_info_wrapper!(run_script_time, "GetTotalScriptPlusExecutionTime");
run_info_wrapper!(run_noop, "Pause");

/// Local service surface for the remaining startup services. They report the
/// already-known local session facts without making network requests.
fn misc_service(lua: &Lua, ctx: &WorldRef, id: &str, method: &str, _args: MultiValue) -> mlua::Result<MultiValue> {
    let class = ctx
        .borrow()
        .nodes
        .get(id)
        .map(|n| n.class_name.clone())
        .unwrap_or_default();
    let mut out = MultiValue::new();
    match (class.as_str(), method) {
        ("Teams", "GetTeams" | "getTeams") => {
            // Single array return, like the engine.
            let table = lua.create_table()?;
            let mut index = 0;
            for child in world::children(ctx, id) {
                if world::is_a(ctx, &child, "Team") {
                    index += 1;
                    table.set(index, world::instance(lua, ctx, &child)?)?;
                }
            }
            out.push_back(Value::Table(table));
        }
        ("Teams", "RebalanceTeams") => {}
        ("ReplicatedFirst", "IsDefaultLoadingGuiRemoved" | "IsFinishedReplicating") => {
            out.push_back(Value::Boolean(false))
        }
        ("ReplicatedFirst", "RemoveDefaultLoadingScreen" | "SetDefaultLoadingGuiRemoved") => {}
        ("ContentProvider", "Preload" | "PreloadAsync") => {}
        ("ContentProvider", "GetFailedRequests" | "GetDetailedFailedRequests"
        | "ListEncryptedAssets" | "GetDependencyContentIds") => {
            out.push_back(Value::Table(lua.create_table()?))
        }
        ("ContentProvider", "RegisterDefaultEncryptionKey"
        | "RegisterDefaultSessionKey"
        | "RegisterEncryptedAsset"
        | "RegisterSessionEncryptedAsset"
        | "UnregisterDefaultEncryptionKey"
        | "UnregisterEncryptedAsset"
        | "SetBaseUrl") => {}
        ("ContentProvider", "GetAssetFetchStatus") => out.push_back(Value::Integer(0)),
        _ => return Err(expected(&class, method)),
    }
    Ok(out)
}

macro_rules! misc_wrapper {
    ($name:ident, $method:literal) => {
        fn $name(lua: &Lua, ctx: &WorldRef, id: &str, args: MultiValue) -> mlua::Result<MultiValue> {
            misc_service(lua, ctx, id, $method, args)
        }
    };
}
misc_wrapper!(teams_get, "GetTeams");
misc_wrapper!(teams_rebalance, "RebalanceTeams");
misc_wrapper!(replicated_first_removed, "IsDefaultLoadingGuiRemoved");
misc_wrapper!(replicated_first_finished, "IsFinishedReplicating");
misc_wrapper!(replicated_first_remove_loading, "RemoveDefaultLoadingScreen");
misc_wrapper!(replicated_first_set_removed, "SetDefaultLoadingGuiRemoved");
misc_wrapper!(content_preload, "Preload");
misc_wrapper!(content_preload_async, "PreloadAsync");
misc_wrapper!(content_failed, "GetFailedRequests");
misc_wrapper!(content_failed_detailed, "GetDetailedFailedRequests");
misc_wrapper!(content_status, "GetAssetFetchStatus");

fn ensure_user_settings(lua: &Lua, ctx: &WorldRef) -> mlua::Result<String> {
    if let Some(id) = ctx.borrow().services.get("UserSettings").cloned() {
        return Ok(id);
    }
    let settings =
        world::create_internal_node(lua, ctx, "UserSettings", None, serde_json::Map::new())?;
    let settings_id = world::instance_id(&settings).ok_or_else(|| {
        mlua::Error::RuntimeError("could not create local UserSettings model".into())
    })?;
    if let Value::UserData(userdata) = &settings {
        userdata.set("Name", "GlobalBasicSettings")?;
    }
    let mut defaults = json!({
        "MouseSensitivity":1.0,"GamepadCameraSensitivity":1.0,
        "CameraYInverted":false,"IsUsingCameraYInverted":false,
        "IsUsingGamepadCameraSensitivity":false,"VignetteEnabled":true,
        "VRSmoothRotationEnabled":false,"VRThirdPersonFollowCamEnabled":false,
        "Fullscreen":false,"MasterVolume":1.0
    });
    if matches!(lua.globals().get::<Value>("Enum")?, Value::Nil) {
        super::enums::install(lua)?;
    }
    let enums: mlua::Table = lua.globals().get("Enum")?;
    for (enum_type, item_name, property) in [
        (
            "ComputerCameraMovementMode",
            "Default",
            "ComputerCameraMovementMode",
        ),
        ("ComputerMovementMode", "Default", "ComputerMovementMode"),
        (
            "TouchCameraMovementMode",
            "Default",
            "TouchCameraMovementMode",
        ),
        ("TouchMovementMode", "Default", "TouchMovementMode"),
        ("ControlMode", "Classic", "ControlMode"),
        ("RotationType", "MovementRelative", "RotationType"),
    ] {
        let enum_items: mlua::AnyUserData = enums.get::<mlua::AnyUserData>(enum_type)?;
        let item: Value = enum_items.get(item_name)?;
        defaults[property] = values::to_json(lua, &item)?;
    }
    let properties = defaults.as_object().cloned().unwrap_or_default();
    let game_settings =
        world::create_internal_node(lua, ctx, "UserGameSettings", Some(&settings_id), properties)?;
    let game_settings_id = world::instance_id(&game_settings).ok_or_else(|| {
        mlua::Error::RuntimeError("could not create local UserGameSettings".into())
    })?;
    if let Value::UserData(userdata) = &game_settings {
        userdata.set("Name", "GameSettings")?;
    }
    let mut world = ctx.borrow_mut();
    world
        .services
        .insert("UserSettings".into(), settings_id.clone());
    world
        .services
        .insert("UserGameSettings".into(), game_settings_id);
    Ok(settings_id)
}

/// Expose the local defaults for settings properties as read-only values.
fn client_setting_getter(
    lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    key: &str,
) -> mlua::Result<Option<Value>> {
    let value = ctx
        .borrow()
        .nodes
        .get(id)
        .filter(|n| n.class_name == "UserGameSettings")
        .and_then(|n| {
            n.local_overrides
                .get(key)
                .or_else(|| n.properties.get(key))
                .cloned()
        });
    match value {
        Some(value) => values::from_json(lua, &value).map(Some),
        None => Ok(None),
    }
}

fn user_game_settings(ctx: &WorldRef, id: &str) -> mlua::Result<String> {
    if ctx.borrow().nodes.get(id).map(|n| n.class_name.as_str()) != Some("UserGameSettings") {
        return Err(mlua::Error::RuntimeError(
            "UserGameSettings receiver required".into(),
        ));
    }
    Ok(id.to_owned())
}

fn game_settings_int(lua: &Lua, ctx: &WorldRef, id: &str, args: MultiValue) -> mlua::Result<MultiValue> {
    let id = user_game_settings(ctx, id)?;
    if !args.is_empty() {
        return Err(mlua::Error::RuntimeError(
            "GetCameraYInvertValue takes no arguments".into(),
        ));
    }
    let invert = ctx
        .borrow()
        .nodes
        .get(&id)
        .and_then(|n| n.properties.get("CameraYInverted"))
        .and_then(Json::as_bool)
        .unwrap_or(false);
    // 0 = normal, 1 = inverted, 2 = user choice unpinned.
    Ok(MultiValue::from_vec(vec![Value::Integer(if invert { 1 } else { 0 })]))
}
fn game_settings_cap(_l: &Lua, _c: &WorldRef, _i: &str, _a: MultiValue) -> mlua::Result<MultiValue> {
    // Headless runtime is uncapped; report the engine's default 60 cap.
    Ok(MultiValue::from_vec(vec![Value::Integer(60)]))
}
fn game_settings_true(lua: &Lua, _c: &WorldRef, _i: &str, _a: MultiValue) -> mlua::Result<MultiValue> {
    let _ = lua;
    Ok(MultiValue::from_vec(vec![Value::Boolean(true)]))
}
fn game_settings_false(_l: &Lua, _c: &WorldRef, _i: &str, _a: MultiValue) -> mlua::Result<MultiValue> {
    Ok(MultiValue::from_vec(vec![Value::Boolean(false)]))
}
fn game_settings_fullscreen(_l: &Lua, _c: &WorldRef, _i: &str, _a: MultiValue) -> mlua::Result<MultiValue> {
    Ok(MultiValue::from_vec(vec![Value::Boolean(false)]))
}
fn game_settings_noop(_l: &Lua, _c: &WorldRef, _i: &str, _a: MultiValue) -> mlua::Result<MultiValue> {
    Ok(MultiValue::new())
}

fn client_setting_setter(
    lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    key: &str,
    value: Value,
) -> mlua::Result<bool> {
    let node = ctx.borrow().nodes.get(id).cloned();
    let Some(node) = node.filter(|node| node.class_name == "UserGameSettings") else {
        return Ok(false);
    };
    let Some(default) = node
        .local_overrides
        .get(key)
        .or_else(|| node.properties.get(key))
    else {
        return Err(mlua::Error::RuntimeError(format!(
            "property '{key}' is unavailable in this local UserGameSettings fixture"
        )));
    };
    if matches!(
        key,
        "IsUsingCameraYInverted" | "IsUsingGamepadCameraSensitivity"
    ) {
        return Err(mlua::Error::RuntimeError(format!(
            "property '{key}' is read-only"
        )));
    }
    let valid = match default {
        Json::Bool(_) => matches!(value, Value::Boolean(_)),
        Json::Number(_) => matches!(value, Value::Integer(_) | Value::Number(_)),
        Json::Object(object) if object.contains_key("$enum") => {
            let encoded = values::to_json(lua, &value)?;
            encoded.get("$enum").and_then(|entry| entry.get("type"))
                == default.get("$enum").and_then(|entry| entry.get("type"))
        }
        _ => false,
    };
    if !valid {
        return Err(mlua::Error::RuntimeError(format!(
            "invalid value type for UserGameSettings.{key}"
        )));
    }
    Ok(false)
}

fn game_settings(lua: &Lua, ctx: &WorldRef, _: &str, args: MultiValue) -> mlua::Result<MultiValue> {
    if !args.is_empty() {
        return Err(mlua::Error::RuntimeError(
            "UserSettings takes no arguments".into(),
        ));
    }
    let id = ensure_user_settings(lua, ctx)?;
    let mut out = MultiValue::new();
    out.push_back(world::instance(lua, ctx, &id)?);
    Ok(out)
}

macro_rules! method_wrapper {
    ($name:ident, $target:ident, $method:literal) => {
        fn $name(
            lua: &Lua,
            ctx: &WorldRef,
            id: &str,
            args: MultiValue,
        ) -> mlua::Result<MultiValue> {
            $target(lua, ctx, id, $method, args)
        }
    };
}

method_wrapper!(players_get_players, players, "GetPlayers");
method_wrapper!(players_get_players_lower, players, "getPlayers");
method_wrapper!(players_players, players, "players");
method_wrapper!(players_by_user_id, players, "GetPlayerByUserId");
method_wrapper!(players_from_character, players, "GetPlayerFromCharacter");
method_wrapper!(player_distance, player_method, "DistanceFromCharacter");
method_wrapper!(player_mouse, player_method, "GetMouse");
method_wrapper!(http_json_encode, http_method, "JSONEncode");
method_wrapper!(http_json_decode, http_method, "JSONDecode");
method_wrapper!(http_url_encode, http_method, "UrlEncode");
method_wrapper!(test_service_message, test_service, "Message");
method_wrapper!(run_is_client, run_state, "IsClient");
method_wrapper!(run_is_server, run_state, "IsServer");
method_wrapper!(run_is_studio, run_state, "IsStudio");
method_wrapper!(run_is_edit, run_state, "IsEdit");
method_wrapper!(run_is_run_mode, run_state, "IsRunMode");
method_wrapper!(run_is_running, run_state, "IsRunning");
method_wrapper!(collection_add_tag, collection_method, "AddTag");
method_wrapper!(collection_remove_tag, collection_method, "RemoveTag");
method_wrapper!(collection_has_tag, collection_method, "HasTag");
method_wrapper!(collection_get_tags, collection_method, "GetTags");
method_wrapper!(collection_get_tagged, collection_method, "GetTagged");
method_wrapper!(collection_get_all_tags, collection_method, "GetAllTags");
method_wrapper!(
    collection_added_signal,
    collection_method,
    "GetInstanceAddedSignal"
);
method_wrapper!(
    collection_removed_signal,
    collection_method,
    "GetInstanceRemovedSignal"
);
method_wrapper!(instance_add_tag, instance_tag_method, "AddTag");
method_wrapper!(instance_remove_tag, instance_tag_method, "RemoveTag");
method_wrapper!(instance_has_tag, instance_tag_method, "HasTag");
method_wrapper!(instance_get_tags, instance_tag_method, "GetTags");
method_wrapper!(stats_get_value, stats_item_method, "GetValue");
method_wrapper!(stats_get_value_string, stats_item_method, "GetValueString");

fn datamodel_is_loaded(
    _lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    args: MultiValue,
) -> mlua::Result<MultiValue> {
    let world = ctx.borrow();
    let Some(node) = world.nodes.get(id) else {
        return Err(mlua::Error::RuntimeError(
            "IsLoaded requires DataModel".into(),
        ));
    };
    if node.class_name != "DataModel" || node.parent.is_some() {
        return Err(mlua::Error::RuntimeError(
            "IsLoaded requires DataModel".into(),
        ));
    }
    if !args.is_empty() {
        return Err(mlua::Error::RuntimeError(
            "IsLoaded takes no arguments".into(),
        ));
    }
    let loaded = world
        .snapshot
        .get("initial_replication_complete")
        .and_then(Json::as_bool)
        .unwrap_or(false);
    let mut out = MultiValue::new();
    out.push_back(Value::Boolean(loaded));
    Ok(out)
}

fn local_player_id(ctx: &WorldRef) -> Option<String> {
    let world = ctx.borrow();
    let id = world.snapshot.get("local_player")?.as_str()?;
    world
        .nodes
        .get(id)
        .filter(|node| node.class_name == "Player" && !node.destroyed)?;
    Some(id.to_owned())
}

fn player_local_player(
    lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    _key: &str,
) -> mlua::Result<Option<Value>> {
    if ctx.borrow().nodes.get(id).map(|n| n.class_name.as_str()) != Some("Players") {
        return Ok(None);
    }
    match local_player_id(ctx) {
        Some(player) => Ok(Some(world::instance(lua, ctx, &player)?)),
        None => Ok(Some(Value::Nil)),
    }
}

pub fn install(lua: &Lua, ctx: &WorldRef) -> mlua::Result<()> {
    super::material_service::install(ctx);
    let runtime_version = lua.create_function(|_, args: MultiValue| {
        if !args.is_empty() {
            return Err(mlua::Error::RuntimeError(
                "version takes no arguments".into(),
            ));
        }
        Ok(env!("CARGO_PKG_VERSION"))
    })?;
    lua.globals().set("version", runtime_version.clone())?;
    lua.globals().set("Version", runtime_version)?;
    world::register_keyed_getter(ctx, "LocalPlayer", player_local_player);
    world::register_method(ctx, "DataModel", "IsLoaded", datamodel_is_loaded);
    for (name, method) in [
        ("GetPlayers", players_get_players as world::Method),
        ("getPlayers", players_get_players_lower),
        ("players", players_players),
        ("GetPlayerByUserId", players_by_user_id),
        ("GetUserThumbnailAsync", players_thumbnail),
        ("GetPlayerFromCharacter", players_from_character),
    ] {
        world::register_method(ctx, "Players", name, method);
    }
    world::register_method(ctx, "Player", "DistanceFromCharacter", player_distance);
    world::register_method(ctx, "Player", "GetMouse", player_mouse);
    world::register_method(ctx, "HttpService", "JSONEncode", http_json_encode);
    world::register_method(ctx, "HttpService", "JSONDecode", http_json_decode);
    world::register_method(ctx, "HttpService", "UrlEncode", http_url_encode);
    world::register_method(
        ctx,
        "TextService",
        "GetTextSize",
        text_service_get_text_size,
    );
    world::register_method(ctx, "TestService", "Message", test_service_message);
    for (name, method) in [
        ("GetTranslatorForPlayer", loc_translator as world::Method),
        ("GetTranslatorForPlayerAsync", loc_translator_async),
    ] {
        world::register_method(ctx, "LocalizationService", name, method);
    }
    for (name, method) in [
        ("Translate", translator_translate as world::Method),
        ("FormatByKey", translator_format),
    ] {
        world::register_method(ctx, "Translator", name, method);
    }
    for (name, method) in [
        ("IsClient", run_is_client as world::Method),
        ("IsServer", run_is_server),
        ("IsStudio", run_is_studio),
        ("IsEdit", run_is_edit),
        ("IsRunMode", run_is_run_mode),
        ("IsRunning", run_is_running),
        ("GetPredictionStatus", run_prediction_status),
        ("GetPhysicsStepId", run_physics_step),
        ("GetRobloxVersion", run_version),
        ("GetCoreScriptVersion", run_core_version),
        ("GetRobloxClientChannel", run_channel),
        ("GetRobloxGuiFocused", run_gui_focused),
        ("getThrottleFramerateEnabled", run_throttle),
        ("IsResimulating", run_resimulating),
        ("IsTeamTest", run_team_test),
        ("IsTeamCreateViewer", run_team_viewer),
        ("GetControlAndVariantRolloutFlags", run_rollout),
        ("GetRobloxClientIxpEnrolledExperiments", run_ixp),
        ("GetTotalScriptPlusExecutionTime", run_script_time),
        ("Pause", run_noop),
        ("Run", run_noop),
        ("Stop", run_noop),
        ("Reset", run_noop),
        ("Set3dRenderingEnabled", run_noop),
        ("SetPredictionMode", run_noop),
        ("SetRobloxGuiFocused", run_noop),
        ("setThrottleFramerateEnabled", run_noop),
        ("BindToSimulation", run_noop),
        ("BindToSimulationParallel", run_noop),
        ("BindToAnimation", run_noop),
        ("BindToAnimationParallel", run_noop),
    ] {
        world::register_method(ctx, "RunService", name, method);
    }
    for (name, method) in [
        ("GetTeams", teams_get as world::Method),
        ("getTeams", teams_get),
        ("RebalanceTeams", teams_rebalance),
    ] {
        world::register_method(ctx, "Teams", name, method);
    }
    for (name, method) in [
        ("IsDefaultLoadingGuiRemoved", replicated_first_removed as world::Method),
        ("IsFinishedReplicating", replicated_first_finished),
        ("RemoveDefaultLoadingScreen", replicated_first_remove_loading),
        ("SetDefaultLoadingGuiRemoved", replicated_first_set_removed),
    ] {
        world::register_method(ctx, "ReplicatedFirst", name, method);
    }
    for (name, method) in [
        ("Preload", content_preload as world::Method),
        ("PreloadAsync", content_preload_async),
        ("GetFailedRequests", content_failed),
        ("GetDetailedFailedRequests", content_failed_detailed),
        ("GetAssetFetchStatus", content_status),
    ] {
        world::register_method(ctx, "ContentProvider", name, method);
    }
    for (name, method) in [
        ("AddTag", collection_add_tag as world::Method),
        ("RemoveTag", collection_remove_tag),
        ("HasTag", collection_has_tag),
        ("GetTags", collection_get_tags),
        ("GetTagged", collection_get_tagged),
        ("GetAllTags", collection_get_all_tags),
        ("GetInstanceAddedSignal", collection_added_signal),
        ("GetInstanceRemovedSignal", collection_removed_signal),
    ] {
        world::register_method(ctx, "CollectionService", name, method);
    }
    for (name, method) in [
        ("AddTag", instance_add_tag as world::Method),
        ("RemoveTag", instance_remove_tag),
        ("HasTag", instance_has_tag),
        ("GetTags", instance_get_tags),
    ] {
        world::register_method(ctx, "Instance", name, method);
    }
    world::register_method(ctx, "Debris", "AddItem", debris_add_item);
    world::register_method(ctx, "StatsItem", "GetValue", stats_get_value);
    world::register_method(ctx, "StatsItem", "GetValueString", stats_get_value_string);
    world::register_getter(ctx, "Network", stats_network);
    let weak = Rc::downgrade(ctx);
    let stats = lua.create_function(move |lua, _: MultiValue| {
        let ctx = weak
            .upgrade()
            .ok_or_else(|| mlua::Error::RuntimeError("world has been released".into()))?;
        let id = ctx.borrow().services.get("Stats").cloned();
        if let Some(id) = id {
            if !ctx
                .borrow()
                .nodes
                .get(&id)
                .is_some_and(|node| node.class_name == "Stats" && !node.destroyed)
            {
                return Err(mlua::Error::RuntimeError(
                    "Stats service is unavailable".into(),
                ));
            }
            return world::instance(lua, &ctx, &id);
        }
        world::ensure_client_service(lua, &ctx, "Stats")
    })?;
    lua.globals().set("Stats", stats.clone())?;
    lua.globals().set("stats", stats)?;
    for property in [
        "MouseSensitivity",
        "GamepadCameraSensitivity",
        "CameraYInverted",
        "IsUsingCameraYInverted",
        "IsUsingGamepadCameraSensitivity",
        "VignetteEnabled",
        "VRSmoothRotationEnabled",
        "VRThirdPersonFollowCamEnabled",
        "Fullscreen",
        "MasterVolume",
        "ComputerCameraMovementMode",
        "ComputerMovementMode",
        "TouchCameraMovementMode",
        "TouchMovementMode",
        "ControlMode",
        "RotationType",
    ] {
        world::register_keyed_setter(ctx, property, client_setting_setter);
        world::register_keyed_getter(ctx, property, client_setting_getter);
    }
    for (name, method) in [
        ("GetCameraYInvertValue", game_settings_int as world::Method),
        ("GetDefaultFramerateCap", game_settings_cap),
        ("GetOnboardingCompleted", game_settings_true),
        ("GetTutorialState", game_settings_true),
        ("InFullScreen", game_settings_fullscreen),
        ("InStudioMode", game_settings_false),
        ("ResetOnboardingCompleted", game_settings_noop),
        ("SetCameraYInvertVisible", game_settings_noop),
        ("SetGamepadCameraSensitivityVisible", game_settings_noop),
        ("SetOnboardingCompleted", game_settings_noop),
        ("SetTutorialState", game_settings_noop),
    ] {
        world::register_method(ctx, "UserGameSettings", name, method);
    }
    let weak = std::rc::Rc::downgrade(ctx);
    lua.globals().set(
        "UserSettings",
        lua.create_function(move |lua, args: MultiValue| {
            let ctx = weak
                .upgrade()
                .ok_or_else(|| mlua::Error::RuntimeError("world has been released".into()))?;
            let mut out = game_settings(lua, &ctx, "", args)?.into_iter();
            Ok(out.next().unwrap_or(Value::Nil))
        })?,
    )?;
    for (name, class) in [
        ("settings", "GlobalSettings"),
        ("PluginManager", "PluginManager"),
    ] {
        let weak = Rc::downgrade(ctx);
        lua.globals().set(
            name,
            lua.create_function(move |lua, args: MultiValue| {
                if !args.is_empty() {
                    return Err(mlua::Error::RuntimeError(format!(
                        "{name} takes no arguments"
                    )));
                }
                let ctx = weak
                    .upgrade()
                    .ok_or_else(|| mlua::Error::RuntimeError("world has been released".into()))?;
                world::ensure_engine_global_node(lua, &ctx, class)
            })?,
        )?;
    }
    for (name, unavailable) in [
        (
            "GetFFlag",
            "engine flag values are unavailable in this local runtime",
        ),
        (
            "GetFVariable",
            "engine variable values are unavailable in this local runtime",
        ),
    ] {
        world::register_method_closure(ctx, "GlobalSettings", name, move |_, _, _, args| {
            if !matches!(args.front(), Some(Value::String(_))) {
                return Err(mlua::Error::RuntimeError(format!(
                    "{name} expects a string"
                )));
            }
            Err(mlua::Error::RuntimeError(unavailable.into()))
        });
    }
    for name in ["CreatePlugin", "ExportPlace", "ExportSelection"] {
        world::register_method_closure(ctx, "PluginManager", name, |_, _, _, _| {
            Err(mlua::Error::RuntimeError(
                "PluginSecurity is unavailable in this local runtime".into(),
            ))
        });
    }
    Ok(())
}
