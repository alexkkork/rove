//! A shared frame order for the root VM and Actor VMs.
use super::{movement, playback, scheduler, signals, world};
use mlua::{Lua, MultiValue, Value, Thread};
use std::cell::RefCell;
use std::time::{Duration, Instant};

#[derive(Default)]
struct Timings(std::collections::BTreeMap<&'static str, crate::stage_timing::StageTiming>);
fn observe(lua: &Lua, name: &'static str, started: Instant) {
    if lua.app_data_ref::<Timings>().is_none() { lua.set_app_data(Timings::default()); }
    lua.app_data_mut::<Timings>().unwrap().0.entry(name).or_default().observe(started);
}
pub(crate) fn timings(lua: &Lua) -> serde_json::Value {
    lua.app_data_ref::<Timings>().map_or(serde_json::Value::Null, |v| serde_json::json!(v.0))
}

#[derive(Default)]
struct FrameAdmission {
    callbacks: Vec<(Thread, u64)>,
    elapsed: f64,
}

pub(crate) fn step(
    lua: &Lua,
    ctx: &world::WorldRef,
    scheduler: &scheduler::SchedulerRef,
    started: Instant,
    elapsed: f64,
    budget: Duration,
) -> mlua::Result<scheduler::StepReport> {
    let mut admission = lua.remove_app_data::<FrameAdmission>().unwrap_or_default();
    admission.elapsed += elapsed;
    admission.callbacks.retain(|(thread, yields)| {
        scheduler::frame_callback_unfinished(scheduler, thread, *yields)
    });
    if !admission.callbacks.is_empty() {
        // VM preemption is not a script yield. Finish the already admitted
        // frame before producing another batch; ordinary jobs still advance.
        let report = scheduler::step(lua, scheduler, Instant::now(), budget);
        lua.set_app_data(admission);
        return Ok(report);
    }
    let elapsed = admission.elapsed;
    let callbacks = RefCell::new(Vec::new());
    let stage = Instant::now();
    super::bindables::prune_inactive_callers(lua);
    super::remotes::prune_inactive_callers(lua, ctx)?;
    world::resolve_instance_handle_waiters(lua, ctx)?;
    let service = ctx.borrow().services.get("RunService").cloned();
    let fire = |events: &[&str]| -> mlua::Result<()> {
        if let Some(id) = &service {
            for event in events {
                let args = if *event == "Stepped" {
                    MultiValue::from_vec(vec![
                        Value::Number(started.elapsed().as_secs_f64()),
                        Value::Number(elapsed),
                    ])
                } else {
                    MultiValue::from_vec(vec![Value::Number(elapsed)])
                };
                callbacks.borrow_mut().extend(scheduler::frame_callback_batch(scheduler, || {
                    signals::fire(lua, ctx, id, event, args)
                })?);
            }
        }
        Ok(())
    };
    // Reserve execution time for every phase. Unused time carries forward,
    // but a native callback that overruns its slice cannot starve later queues.
    // Keep accounting actual execution time in the returned report.
    observe(lua, "prepare", stage);
    let stage = Instant::now();
    // Frame signal batches (RenderStepped, Stepped, Heartbeat) are where game
    // scripts do their per-frame work; give each phase half the slice so a
    // large connection count finishes instead of aborting mid-callback.
    let reserved = budget / 2;
    let animation = scheduler::step_phase(lua, scheduler, reserved, || fire(&["PreAnimation"]))?;
    observe(lua, "pre_animation", stage);
    let stage = Instant::now();
    playback::step(lua, ctx, elapsed)?;
    observe(lua, "playback", stage);
    let stage = Instant::now();
    let remaining = budget.saturating_sub(animation.execution_time.min(reserved));
    let simulation_allowance = remaining.saturating_sub(reserved);
    let simulation = scheduler::step_phase(lua, scheduler, simulation_allowance, || {
        fire(&["PreSimulation", "Stepped"])
    })?;
    observe(lua, "pre_simulation", stage);
    let stage = Instant::now();
    movement::flush_joint_poses(lua, ctx)?;
    observe(lua, "joints", stage);
    let stage = Instant::now();
    world::refresh_derived_properties(lua)?;
    observe(lua, "derived_properties", stage);
    let stage = Instant::now();
    fire(&["PostSimulation", "Heartbeat", "PreRender", "RenderStepped"])?;
    super::render_steps::replay(lua, elapsed)?;
    observe(lua, "post_signals", stage);
    let stage = Instant::now();
    let mut result = scheduler::step(
        lua,
        scheduler,
        Instant::now(),
        remaining.saturating_sub(simulation.execution_time.min(simulation_allowance)),
    );
    observe(lua, "ordinary_tasks", stage);
    for phase in [animation, simulation] {
        result.execution_time += phase.execution_time;
        result.resumed += phase.resumed;
        result.completed += phase.completed;
        result.preempted += phase.preempted;
        result.errors.extend(phase.errors);
    }
    lua.set_app_data(FrameAdmission { callbacks: callbacks.into_inner(), elapsed: 0.0 });
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc, time::Duration};
    #[test]
    fn unfinished_frame_callbacks_drain_before_admitting_another_frame() {
        let lua = Lua::new();
        let scheduler = Rc::new(RefCell::new(scheduler::Scheduler::new()));
        scheduler::install(&lua, &scheduler).unwrap();
        let ctx = Rc::new(RefCell::new(world::World::from_snapshot(&serde_json::json!({
            "version":1,"sequence":0,"services":{"RunService":"run"},"nodes":[
                {"id":"game","class_name":"DataModel","name":"game"},
                {"id":"run","class_name":"RunService","name":"RunService","parent":"game"}
            ]
        }), None).unwrap()));
        super::super::install_bindings(&lua, &ctx).unwrap();
        lua.globals().set("native_work", lua.create_function(|_, ()| {
            std::thread::sleep(Duration::from_millis(3)); Ok(())
        }).unwrap()).unwrap();
        lua.load(r#"
            counts={}; ordinary=0
            for i=1,8 do
                counts[i]=0
                game:GetService('RunService').PreSimulation:Connect(function()
                    native_work(); counts[i]+=1
                end)
            end
            task.spawn(function() ordinary=ordinary+1 end)
        "#).exec().unwrap();
        for _ in 0..50 {
            let report = step(&lua,&ctx,&scheduler,Instant::now(),0.01,Duration::from_millis(2)).unwrap();
            assert!(report.errors.is_empty(), "{:?}", report.errors);
            assert!(scheduler::snapshot(&scheduler).active_tasks <= 9);
        }
        lua.load("assert(ordinary==1); for _,n in counts do assert(n>=3) end").exec().unwrap();
    }

    #[test]
    fn voluntarily_yielding_frame_callbacks_do_not_hold_frame_admission() {
        let lua = Lua::new();
        let scheduler = Rc::new(RefCell::new(scheduler::Scheduler::new()));
        scheduler::install(&lua, &scheduler).unwrap();
        let ctx = Rc::new(RefCell::new(world::World::from_snapshot(&serde_json::json!({
            "version":1,"sequence":0,"services":{"RunService":"run"},"nodes":[
                {"id":"game","class_name":"DataModel","name":"game"},
                {"id":"run","class_name":"RunService","name":"RunService","parent":"game"}
            ]
        }), None).unwrap()));
        super::super::install_bindings(&lua,&ctx).unwrap();
        lua.load("count=0; game:GetService('RunService').Heartbeat:Connect(function() count+=1; task.wait(10) end)")
            .exec().unwrap();
        for _ in 0..4 {
            let report=step(&lua,&ctx,&scheduler,Instant::now(),0.01,Duration::from_millis(20)).unwrap();
            assert!(report.errors.is_empty());
        }
        lua.load("assert(count==4)").exec().unwrap();
    }

    #[test]
    fn native_phase_overrun_does_not_starve_ordinary_tasks_or_heartbeat() {
        let lua = Lua::new();
        let scheduler = Rc::new(RefCell::new(scheduler::Scheduler::new()));
        scheduler::install(&lua, &scheduler).unwrap();
        let ctx = Rc::new(RefCell::new(world::World::from_snapshot(&serde_json::json!({
            "version":1,"sequence":0,"services":{"RunService":"run"},"nodes":[
                {"id":"game","class_name":"DataModel","name":"game"},
                {"id":"run","class_name":"RunService","name":"RunService","parent":"game"}
            ]
        }), None).unwrap()));
        super::super::install_bindings(&lua, &ctx).unwrap();
        lua.globals().set("native_overrun", lua.create_function(|_, ()| {
            // A native engine call cannot be interrupted mid-call. Deliberately
            // exceed the whole frame slice to exercise the starvation case.
            std::thread::sleep(Duration::from_millis(20));
            Ok(())
        }).unwrap()).unwrap();
        lua.load(r#"
            ordinary_count=0; simulation_count=0; heartbeat_count=0
            local run=game:GetService('RunService')
            run.PreAnimation:Connect(native_overrun)
            run.PreSimulation:Connect(function() simulation_count+=1 end)
            run.Heartbeat:Connect(function() heartbeat_count+=1 end)
            task.spawn(function() ordinary_count+=1 end)
        "#).exec().unwrap();
        let report = step(&lua, &ctx, &scheduler, Instant::now(), 1.0/60.0,
            Duration::from_millis(6)).unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert!(report.execution_time >= Duration::from_millis(20));
        lua.load("assert(ordinary_count==1); assert(simulation_count==1); assert(heartbeat_count==1)")
            .exec().unwrap();
    }

    #[test]
    fn phase_callbacks_do_not_wake_ordinary_waiters_early() {
        let lua = Lua::new();
        let scheduler = Rc::new(RefCell::new(scheduler::Scheduler::new()));
        scheduler::install(&lua, &scheduler).unwrap();
        lua.load("order={};task.spawn(function() table.insert(order,'ordinary') end);task.delay(0,function() table.insert(order,'sleeper') end)").exec().unwrap();
        scheduler::step_phase(&lua, &scheduler, Duration::from_millis(30), || {
            scheduler::enqueue(
                &lua,
                lua.load("table.insert(order,'phase')").into_function()?,
                MultiValue::new(),
            )?;
            Ok(())
        })
        .unwrap();
        lua.load("assert(table.concat(order,',')=='phase')")
            .exec()
            .unwrap();
        scheduler::step(&lua, &scheduler, Instant::now(), Duration::from_millis(30));
        lua.load("assert(table.concat(order,',')=='phase,ordinary,sleeper')")
            .exec()
            .unwrap();
    }
    #[test]
    fn phase_enqueue_failure_restores_existing_and_partial_callback_queues() {
        let lua = Lua::new();
        let scheduler = Rc::new(RefCell::new(scheduler::Scheduler::new()));
        scheduler::install(&lua, &scheduler).unwrap();
        lua.load(
            "count=0;task.spawn(function() count+=1 end);task.delay(0,function() count+=10 end)",
        )
        .exec()
        .unwrap();
        let result = scheduler::step_phase(&lua, &scheduler, Duration::ZERO, || {
            scheduler::enqueue(
                &lua,
                lua.load("count+=100").into_function()?,
                MultiValue::new(),
            )?;
            Err(mlua::Error::RuntimeError("enqueue failed".into()))
        });
        assert!(result.is_err());
        assert_eq!(scheduler::snapshot(&scheduler).active_tasks, 3);
        scheduler::step(&lua, &scheduler, Instant::now(), Duration::from_millis(30));
        lua.load("assert(count==111)").exec().unwrap();
    }
}
