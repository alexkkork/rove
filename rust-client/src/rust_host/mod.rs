//! the rust-owned engine and host around the official luau vm.
//! rust-host is the default; the old host is an explicit comparison build.

pub use rove_runtime_sys::mlua as lua_api;

pub mod actor_io;
pub mod actor_shadow;
mod actor_movement;
pub mod actors;
pub(crate) mod attribute_events;
pub(crate) mod property_events;
pub mod bindables;
pub mod camera;
pub mod cframe;
pub(crate) mod collection_events;
pub(crate) mod curves;
pub mod closures;
pub mod drawing;
pub mod enums;
pub mod errors;
pub(crate) mod frame_phases;
pub mod geometry;
pub mod gui;
pub mod host_io_bindings;
pub(crate) mod hierarchy_events;
pub(crate) mod humanoid_health;
pub(crate) mod humanoid_state;
pub mod idle;
pub mod input;
pub mod instance_cache;
pub mod instance_helpers;
pub mod integrity;
pub mod interactions;
pub(crate) mod joint_graph;
pub mod lifecycle;
pub mod misc_values;
pub mod movement;
pub mod numeric;
pub mod playback;
pub(crate) mod player_lifecycle;
pub(crate) mod program_context;
pub mod remotes;
pub mod input_actions;
pub mod render_steps;
pub mod scheduler;
pub mod script_threads;
pub mod sequences;
pub mod service_requests;
pub mod services;
mod material_service;
mod material_history;
mod assembly_history;
pub mod session;
pub mod shared_tables;
pub mod signals;
pub mod source_helpers;
mod runtime_identity;
mod script_bytes;
mod decompile_binding;
mod reflected_signals;
mod authored_budget;
pub mod standalone;
pub mod temporal;
pub mod value_types;
pub mod values;
pub mod world;
pub(crate) mod world_aliases;

pub fn install_bindings(lua: &mlua::Lua, ctx: &world::WorldRef) -> mlua::Result<()> {
    runtime_identity::install(lua, ctx)?;
    errors::install(lua)?;
    integrity::install(lua)?;
    enums::install(lua)?;
    numeric::install(lua)?;
    cframe::install(lua)?;
    closures::install(lua)?;
    value_types::install(lua)?;
    sequences::install(lua)?;
    temporal::install(lua)?;
    signals::install(lua, ctx)?;
    idle::install(lua, ctx)?;
    world::install(lua, ctx)?;
    curves::install(ctx);
    bindables::install(lua, ctx)?;
    misc_values::install(lua, ctx)?;
    geometry::install(lua, ctx)?;
    movement::install(lua, ctx)?;
    joint_graph::install(ctx);
    input::install(lua, ctx)?;
    render_steps::install(lua, ctx)?;
    input_actions::install(lua, ctx)?;
    gui::install(lua, ctx)?;
    camera::install(lua, ctx)?;
    drawing::install(lua, ctx)?;
    playback::install(lua, ctx)?;
    services::install(lua, ctx)?;
    service_requests::install(lua, ctx)?;
    source_helpers::install(lua, ctx)?;
    decompile_binding::install(lua, ctx)?;
    host_io_bindings::install(lua, ctx)?;
    remotes::install(lua, ctx)?;
    instance_helpers::install(lua, ctx)?;
    interactions::install(lua, ctx)?;
    reflected_signals::install(lua, ctx)?;
    shared_tables::install(lua, ctx, shared_tables::SharedTableRegistry::default())?;
    lifecycle::install(lua, ctx)?;
    script_bytes::install(lua, ctx)?;
    script_threads::install(lua)?;
    Ok(())
}

#[cfg(test)]
#[path = "input_actions_tests.rs"]
mod input_actions_tests;
#[cfg(test)]
#[path = "size_probe_tests.rs"]
mod size_probe_tests;
#[cfg(test)]
#[path = "../asset_crypto_live_tests.rs"]
mod asset_crypto_live_tests;
#[cfg(test)]
mod bindables_tests;
#[cfg(test)]
mod closures_tests;
#[cfg(test)]
mod errors_tests;
#[cfg(test)]
mod host_io_tests;
#[cfg(test)]
mod host_tests;
#[cfg(test)]
mod instance_helpers_tests;
#[cfg(test)]
mod numeric_tests;
#[cfg(test)]
mod services_tests;
#[cfg(test)]
mod signals_tests;
#[cfg(test)]
mod world_tests;
