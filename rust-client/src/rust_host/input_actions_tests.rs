use crate::rust_host::{
    enums, input_actions, scheduler::{self, Scheduler}, signals, value_types,
    world::{self, World, WorldRef},
};
use mlua::Lua;
use serde_json::json;
use std::{cell::RefCell, rc::Rc};

fn fixture() -> WorldRef {
    Rc::new(RefCell::new(
        World::from_snapshot(
            &json!({"version":1,"sequence":0,"services":{},"nodes":[
                {"id":"game","class_name":"DataModel","name":"game"},
                {"id":"ctx","class_name":"InputContext","name":"ctx","parent":"game","properties":{},"server_owned":true},
                {"id":"act","class_name":"InputAction","name":"act","parent":"ctx","properties":{},"server_owned":true},
                {"id":"bind","class_name":"InputBinding","name":"b","parent":"act","properties":{},"server_owned":true}
            ]}),
            None,
        )
        .unwrap(),
    ))
}

#[test]
fn bool_action_state_fires_and_reports_typed_value() -> mlua::Result<()> {
    let lua = Lua::new();
    enums::install(&lua)?;
    value_types::install(&lua)?;
    let ctx = fixture();
    world::install(&lua, &ctx)?;
    let sched = Rc::new(RefCell::new(Scheduler::new()));
    scheduler::install(&lua, &sched)?;
    let sched = Rc::new(RefCell::new(Scheduler::new()));
    scheduler::install(&lua, &sched)?;
    signals::install(&lua, &ctx)?;
    input_actions::install(&lua, &ctx)?;
    lua.globals().set("act", world::instance(&lua, &ctx, "act")?)?;

    assert!(!lua.load("return act:GetState()").eval::<bool>()?);
    assert!(!lua.load("return act.BoolState").eval::<bool>()?);
    lua.load(
            r#"
            act.Pressed:Connect(function() shared_seen = (shared_seen or 0) + 1 end)
        "#,
    )
    .exec()?;
    let state = lua
        .load(
            r#"
            shared_seen = 0
            act:Fire(true)
            return act:GetState(), act.BoolState
        "#,
        )
        .eval::<(bool, bool)>()?;
    assert_eq!(state, (true, true));
    // Signal callbacks run on the scheduler, so the count lands after a step.
    scheduler::step(&lua, &sched, std::time::Instant::now(), std::time::Duration::from_millis(50));
    assert_eq!(lua.load("return shared_seen").eval::<i64>()?, 1);

    let released = lua
        .load("act:Fire(false); return act:GetState()")
        .eval::<bool>()?;
    assert!(!released);
    Ok(())
}

#[test]
fn key_dispatch_matches_keycode_bindings_and_is_idempotent() -> mlua::Result<()> {
    let lua = Lua::new();
    enums::install(&lua)?;
    value_types::install(&lua)?;
    let ctx = fixture();
    world::install(&lua, &ctx)?;
    let sched = Rc::new(RefCell::new(Scheduler::new()));
    scheduler::install(&lua, &sched)?;
    signals::install(&lua, &ctx)?;
    input_actions::install(&lua, &ctx)?;
    lua.globals().set(
        "key",
        crate::rust_host::enums::enum_item(&lua, "KeyCode", "E")?,
    )?;
    world::set_property(&lua, &ctx, "bind", "KeyCode", json!({"$enum":{"type":"KeyCode","name":"E","value":101}}))?;

    println!("children {:?}", world::children(&ctx, "act"));
    println!("bindings {:?}", world::get_property(&ctx, "bind", "KeyCode"));
    let fired = input_actions::dispatch_key(&ctx, "E", true, false);
    println!("fired {fired:?}");
    assert_eq!(fired, vec!["act".to_string()]);
    assert_eq!(
        world::get_property(&ctx, "act", "BoolState")?.as_bool(),
        Some(true)
    );
    // Repeat without `repeat` stays a no-op.
    assert!(input_actions::dispatch_key(&ctx, "E", true, false).is_empty());
    assert!(!input_actions::dispatch_key(&ctx, "E", false, false).is_empty());
    assert_eq!(
        world::get_property(&ctx, "act", "BoolState")?.as_bool(),
        Some(false)
    );
    Ok(())
}
