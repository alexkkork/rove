//! the rust client pieces, plus the bridge to the pinned official luau vm.
extern crate self as headless_luau;
pub(crate) const EXECUTOR_NAME: &str = "rove";
pub(crate) const EXECUTOR_VERSION: &str = "1.00";
pub const EXECUTOR_USER_AGENT: &str = "rove 1.00";
pub mod account_auth;
pub mod allocation;
pub mod client_version;
pub mod asset_crypto;
mod animation_queue;
pub mod bytecode_format;
pub mod capture_replay;
pub mod dashboard;
mod codecs;
pub mod connection_probe;
mod decompiler;
pub mod engine_attributes;
// Only the legacy C++ host installs callback tables. The Rust host calls the
// shared engine operations directly and must not link those old entry points.
#[cfg(not(feature = "rust-host"))]
pub mod engine_bridge;
pub mod engine_classes;
pub mod engine_color3;
pub mod engine_datatypes;
pub mod engine_datetime;
pub mod engine_physical_properties;
pub mod engine_services;
mod engine_text_measure;
pub mod engine_vector2;
pub mod engine_world_projection;
pub mod engine_world_view;
mod host_io;
pub mod input_bridge;
pub mod join_cli;
pub mod join_coordinator;
mod character_children;
pub mod live_physics;
mod live_ping;
pub mod live_runtime;
pub mod live_session;
pub mod login;
pub mod movement_math;
mod movement_queue;
pub mod native;
mod network;
mod memory_diagnostics;
mod memory_maintenance;
pub mod memory_budget;
pub mod physics_codec;
pub mod physics_properties;
pub mod physics_sender;
mod owned_part_poses;
mod owned_part_dynamics;
mod owned_part_scene;
mod isr_sender;
mod isr_clock;
mod received_world_ready;
pub mod ping_responder;
pub mod program_check;
pub mod render_assets;
pub mod render_projection;
pub mod runtime_owner;
pub mod runtime_sources;
#[cfg(feature = "rust-host")]
pub mod rust_host;
pub mod session_integrity;
pub mod session_ui;
mod test_limits;
pub mod ui_server;
pub mod animation_assets;

mod script_catalog;

mod stage_timing;
mod attribute_receipts;
