//! `RunService:BindToRenderStep` storage. Bound callbacks replay every render
//! frame in ascending priority order, ahead of ordinary frame tasks.
use super::{scheduler, world::WorldRef};
use mlua::{Function, Lua, MultiValue, Value};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
};

const MAX_BOUND: usize = 128;
const MIN_PRIORITY: i64 = 0;
const MAX_PRIORITY: i64 = 2_000;

struct Bound {
    name: String,
    priority: i64,
    callback: Function,
}

#[derive(Default)]
struct Steps {
    /// Keyed by priority so replay is already ordered; names stay unique.
    items: BTreeMap<(i64, String), Bound>,
}

struct StepsRef(Rc<RefCell<Steps>>);

fn state(lua: &Lua) -> Rc<RefCell<Steps>> {
    if lua.app_data_ref::<StepsRef>().is_none() {
        lua.set_app_data(StepsRef(Rc::new(RefCell::new(Steps::default()))));
    }
    lua.app_data_ref::<StepsRef>().expect("render steps installed").0.clone()
}

fn error(message: &str) -> mlua::Error {
    mlua::Error::RuntimeError(message.into())
}

fn bind(lua: &Lua, ctx: &WorldRef, id: &str, args: MultiValue) -> mlua::Result<MultiValue> {
    if ctx.borrow().nodes.get(id).map(|n| n.class_name.as_str()) != Some("RunService") {
        return Err(error("BindToRenderStep requires RunService"));
    }
    let mut args = args.into_iter();
    let name = match args.next() {
        Some(Value::String(s)) => s.to_str()?.to_string(),
        _ => return Err(error("BindToRenderStep expects a name string")),
    };
    if name.len() > 256 || name.is_empty() {
        return Err(error("BindToRenderStep name must be 1..256 bytes"));
    }
    let priority = match args.next() {
        Some(Value::Integer(v)) => v,
        Some(Value::Number(v)) if v.fract() == 0. => v as i64,
        _ => return Err(error("BindToRenderStep expects an integer priority")),
    };
    if !(MIN_PRIORITY..=MAX_PRIORITY).contains(&priority) {
        return Err(error("BindToRenderStep priority must be 0..2000"));
    }
    let callback = match args.next() {
        Some(Value::Function(f)) => f,
        _ => return Err(error("BindToRenderStep expects a callback function")),
    };
    let steps = state(lua);
    let mut steps = steps.borrow_mut();
    steps.items.retain(|(_, n), _| n != &name);
    if steps.items.len() >= MAX_BOUND {
        return Err(error("BindToRenderStep limit exceeded"));
    }
    steps.items.insert(
        (priority, name.clone()),
        Bound {
            name,
            priority,
            callback,
        },
    );
    Ok(MultiValue::new())
}

fn unbind(lua: &Lua, ctx: &WorldRef, id: &str, args: MultiValue) -> mlua::Result<MultiValue> {
    if ctx.borrow().nodes.get(id).map(|n| n.class_name.as_str()) != Some("RunService") {
        return Err(error("UnbindFromRenderStep requires RunService"));
    }
    let mut args = args.into_iter();
    match args.next() {
        Some(Value::String(s)) => {
            let name = s.to_str()?.to_string();
            let steps = state(lua);
            steps.borrow_mut().items.retain(|(_, n), _| n != &name);
        }
        _ => return Err(error("UnbindFromRenderStep expects a name string")),
    }
    Ok(MultiValue::new())
}

fn unbind_all(lua: &Lua, _ctx: &WorldRef, _id: &str, _args: MultiValue) -> mlua::Result<MultiValue> {
    state(lua).borrow_mut().items.clear();
    Ok(MultiValue::new())
}

fn bound_names(lua: &Lua, _ctx: &WorldRef, _id: &str, _args: MultiValue) -> mlua::Result<MultiValue> {
    let steps = state(lua);
    let steps = steps.borrow();
    let mut out = MultiValue::new();
    for bound in steps.items.values() {
        out.push_back(Value::String(lua.create_string(&bound.name)?));
    }
    Ok(out)
}

/// Enqueue every bound callback for this frame in priority order.
pub(crate) fn replay(lua: &Lua, elapsed: f64) -> mlua::Result<()> {
    let callbacks = {
        let steps = state(lua);
        let steps = steps.borrow();
        steps
            .items
            .values()
            .map(|b| (b.priority, b.callback.clone()))
            .collect::<Vec<_>>()
    };
    for (priority, callback) in callbacks {
        let _ = priority;
        scheduler::enqueue(
            lua,
            callback,
            MultiValue::from_vec(vec![Value::Number(elapsed)]),
        )?;
    }
    Ok(())
}

pub fn install(lua: &Lua, ctx: &WorldRef) -> mlua::Result<()> {
    state(lua);
    super::world::register_method(ctx, "RunService", "BindToRenderStep", bind);
    super::world::register_method(ctx, "RunService", "UnbindFromRenderStep", unbind);
    super::world::register_method(ctx, "RunService", "UnbindAllRenderSteps", unbind_all);
    super::world::register_method(ctx, "RunService", "GetBoundRenderSteps", bound_names);
    Ok(())
}
