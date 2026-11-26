//! New-style `InputAction`/`InputContext` bindings. State is local to this
//! runtime: bindings declare the keys, `Fire` and the input bridge drive
//! state, and `GetState` returns the typed value for the action's `Type`.
use super::{
    enums, signals,
    world::{self, WorldRef},
};
use mlua::{Lua, MultiValue, Value};
use serde_json::{json, Value as Json};

const MAX_STATE_PUSHES: usize = 64;

fn error(message: &str) -> mlua::Error {
    mlua::Error::RuntimeError(message.into())
}

/// Actions store state directly in their typed property so normal property
/// reads and the API agree.
fn state_key(kind: i64) -> &'static str {
    match kind {
        1 => "Direction1DState",
        2 => "Direction2DState",
        3 => "Direction3DState",
        4 => "ViewportPositionState",
        _ => "BoolState",
    }
}

fn action_type(ctx: &WorldRef, id: &str) -> mlua::Result<i64> {
    Ok(world::get_property(ctx, id, "Type")
        .ok()
        .and_then(|v| {
            v.pointer("/$enum/value")
                .and_then(Json::as_i64)
                .or_else(|| v.as_i64())
        })
        .unwrap_or(0))
}

/// Zero value for the action's declared type.
fn empty_state(kind: i64) -> Json {
    match kind {
        1 => json!(0.),
        2 => json!({"$vector2":[0.,0.]}),
        3 => json!({"$vector3":[0.,0.,0.]}),
        4 => json!({"$vector2":[0.,0.]}),
        _ => json!(false),
    }
}

fn read_state(ctx: &WorldRef, id: &str) -> mlua::Result<Json> {
    let kind = action_type(ctx, id)?;
    let stored = ctx
        .borrow()
        .nodes
        .get(id)
        .and_then(|n| n.local_overrides.get(state_key(kind)).cloned());
    Ok(stored.unwrap_or_else(|| empty_state(kind)))
}

/// Validate a state value against the action type and store it.
fn write_state(ctx: &WorldRef, id: &str, kind: i64, value: Json) -> mlua::Result<Json> {
    let valid = match (kind, &value) {
        (0, Json::Bool(_)) => true,
        (1, v) => v.as_f64().is_some_and(f64::is_finite),
        (2 | 4, v) => v
            .get("$vector2")
            .and_then(Json::as_array)
            .is_some_and(|a| a.len() == 2),
        (3, v) => v
            .get("$vector3")
            .and_then(Json::as_array)
            .is_some_and(|a| a.len() == 3),
        _ => false,
    };
    if !valid {
        return Err(error("action state does not match the action Type"));
    }
    let mut world = ctx.borrow_mut();
    let node = world.nodes.get_mut(id).ok_or_else(|| error("action unavailable"))?;
    node.local_overrides.insert(state_key(kind).into(), value.clone());
    Ok(value)
}

fn get_state(lua: &Lua, ctx: &WorldRef, id: &str, args: MultiValue) -> mlua::Result<MultiValue> {
    if !args.is_empty() {
        return Err(error("GetState takes no arguments"));
    }
    if !world::is_a(ctx, id, "InputAction") {
        return Err(error("InputAction receiver required"));
    }
    let value = read_state(ctx, id)?;
    Ok(MultiValue::from_vec(vec![super::values::from_json(lua, &value)?]))
}

fn fire(lua: &Lua, ctx: &WorldRef, id: &str, args: MultiValue) -> mlua::Result<MultiValue> {
    if !world::is_a(ctx, id, "InputAction") {
        return Err(error("InputAction receiver required"));
    }
    let kind = action_type(ctx, id)?;
    let requested = match args.into_iter().next() {
        Some(Value::Nil) | None => empty_state(kind),
        Some(value) => super::values::to_json(lua, &value)?,
    };
    let value = write_state(ctx, id, kind, requested)?;
    let encoded = super::values::from_json(lua, &value)?;
    if kind == 0 {
        let pressed = value.as_bool().unwrap_or(false);
        signals::fire(
            lua,
            ctx,
            id,
            if pressed { "Pressed" } else { "Released" },
            MultiValue::new(),
        )?;
    }
    signals::fire(
        lua,
        ctx,
        id,
        "StateChanged",
        MultiValue::from_vec(vec![encoded]),
    )?;
    Ok(MultiValue::new())
}

fn bindings(ctx: &WorldRef, id: &str) -> Vec<String> {
    world::children(ctx, id)
        .into_iter()
        .filter(|child| world::is_a(ctx, child, "InputBinding"))
        .collect()
}

fn get_bindings(lua: &Lua, ctx: &WorldRef, id: &str, _args: MultiValue) -> mlua::Result<MultiValue> {
    if !world::is_a(ctx, id, "InputAction") {
        return Err(error("InputAction receiver required"));
    }
    let mut out = MultiValue::new();
    for child in bindings(ctx, id) {
        out.push_back(world::instance(lua, ctx, &child)?);
    }
    Ok(out)
}

fn preferred_bindings(lua: &Lua, ctx: &WorldRef, id: &str, args: MultiValue) -> mlua::Result<MultiValue> {
    let mut out = MultiValue::new();
    let count = args
        .into_iter()
        .next()
        .and_then(|v| match v {
            Value::Integer(n) => Some(n),
            _ => None,
        })
        .unwrap_or(i32::MAX as i64)
        .clamp(0, 1024) as usize;
    if !world::is_a(ctx, id, "InputAction") {
        return Ok(out);
    }
    for child in bindings(ctx, id).into_iter().take(count) {
        out.push_back(world::instance(lua, ctx, &child)?);
    }
    Ok(out)
}

fn context_actions(lua: &Lua, ctx: &WorldRef, id: &str, _args: MultiValue) -> mlua::Result<MultiValue> {
    if !world::is_a(ctx, id, "InputContext") {
        return Err(error("InputContext receiver required"));
    }
    let mut out = MultiValue::new();
    for child in world::children(ctx, id) {
        if world::is_a(ctx, &child, "InputAction") {
            out.push_back(world::instance(lua, ctx, &child)?);
        }
    }
    Ok(out)
}

fn key_name(value: &Json) -> Option<String> {
    value
        .get("$enum")
        .and_then(|e| e.get("name"))
        .and_then(Json::as_str)
        .map(str::to_owned)
}

fn action_name(name: &str) -> &str {
    name
}

/// Drive every enabled InputAction whose bindings cover `key` in `ctx`. Called
/// from the input bridge for real and synthetic key events.
pub fn dispatch_key(ctx: &WorldRef, key: &str, down: bool, repeat: bool) -> Vec<String> {
    let ids = world::ordered_matching_ids(ctx, |class| class == "InputAction");
    let mut fired = Vec::new();
    for id in ids {
        if ctx.borrow().nodes.get(&id).is_none_or(|n| n.destroyed) {
            continue;
        }
        let enabled = world::get_property(ctx, &id, "Enabled")
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        if !enabled {
            continue;
        }
        let matched = bindings(ctx, &id).into_iter().any(|b| {
            ["KeyCode", "Up", "Down", "Left", "Right", "Forward", "Backward"]
                .iter()
                .any(|prop| {
                    world::get_property(ctx, &b, prop)
                        .ok()
                        .and_then(|v| key_name(&v))
                        .is_some_and(|k| k == key)
                })
        });
        if !matched {
            continue;
        }
        let pressed = read_state(ctx, &id).ok().and_then(|v| v.as_bool()).unwrap_or(false);
        if pressed == down && !repeat {
            continue;
        }
        let _ = write_state(ctx, &id, 0, json!(down));
        fired.push(id);
    }
    fired
}

pub fn install(lua: &Lua, ctx: &WorldRef) -> mlua::Result<()> {
    let _ = MAX_STATE_PUSHES;
    for (name, func) in [
        ("GetState", get_state as world::Method),
        ("Fire", fire),
        ("GetInputBindings", get_bindings),
        ("GetPreferredBindingList", preferred_bindings),
    ] {
        world::register_method(ctx, "InputAction", name, func);
    }
    world::register_method(ctx, "InputContext", "GetInputActions", context_actions);
    fn action_event(
        lua: &Lua,
        ctx: &WorldRef,
        id: &str,
        key: &str,
    ) -> mlua::Result<Option<Value>> {
        if !world::is_a(ctx, id, "InputAction") {
            return Ok(None);
        }
        signals::signal(lua, ctx, id, key).map(Some)
    }
    for name in [
        "Pressed",
        "Released",
        "StateChanged",
        "InputBindingsChanged",
    ] {
        world::register_keyed_getter(ctx, name, action_event);
    }
    world::register_keyed_getter(ctx, "InputActionsChanged", |lua, ctx, id, _key| {
        if !world::is_a(ctx, id, "InputContext") {
            return Ok(None);
        }
        signals::signal(lua, ctx, id, "InputActionsChanged").map(Some)
    });
    // Typed state properties mirror the last Fire/GetState value.
    for key in [
        "BoolState",
        "Direction1DState",
        "Direction2DState",
        "Direction3DState",
        "ViewportPositionState",
    ] {
        world::register_keyed_getter(ctx, key, |lua, ctx, id, key| {
            if !world::is_a(ctx, id, "InputAction") {
                return Ok(None);
            }
            let kind = match key {
                "BoolState" => 0,
                "Direction1DState" => 1,
                "Direction2DState" => 2,
                "Direction3DState" => 3,
                _ => 4,
            };
            super::values::from_json(lua, &read_state(ctx, id)?).map(Some)
        });
    }
    let _ = action_name;
    let _ = enums::install;
    Ok(())
}
