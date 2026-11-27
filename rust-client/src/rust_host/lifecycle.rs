//! Local script lifecycle for one official Luau VM.
//!
//! This module consumes already-admitted source catalog entries, compiles them
//! with the integrity loader, and schedules only eligible owned client scripts.
//! It never fetches scripts or authenticates a Roblox session.

use mlua::{thread::ThreadStatus, Function, Lua, MultiValue, RegistryKey, Table, Thread, Value};
use serde_json::{json, Map, Value as Json};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::{Rc, Weak},
};

use super::{
    scheduler,
    world::{self, WorldRef},
};

const MAX_CATALOG_ENTRIES: usize = 16384;
const MAX_CATALOG_BYTES: usize = 16 * 1024 * 1024;
const MAX_SOURCE: usize = 128 * 1024;
const MAX_BUNDLE_ENTRIES: usize = 256;
const MAX_BUNDLE_BYTES: usize = 2 * 1024 * 1024;
const MAX_LOCAL_SOURCES_BYTES: usize = 4 * 1024 * 1024;
const MAX_COMPILED_SCRIPT_CLOSURES: usize = 512;
const MAX_COMPILED_SCRIPT_BYTES: usize = 8 * 1024 * 1024;
const BUILTIN_PROVENANCE: &str = "pinned_builtin_luau_0_705";
const BUILTIN_COMMIT: &str = "93c83a46d7c6c1b040e58e82ef167340a7db1aef";

type Result<T> = mlua::Result<T>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ModuleState {
    Loading,
    Loaded,
    Failed,
}

struct ModuleEntry {
    state: ModuleState,
    value: Option<RegistryKey>,
    environment: Option<RegistryKey>,
    error: Option<String>,
    owner_thread: Option<usize>,
    waiters: Vec<Thread>,
    waiting_for: Option<String>,
}

struct ScriptEntry {
    id: String,
    thread: Option<Thread>,
    environment: RegistryKey,
    state: String,
    error: Option<String>,
    active: bool,
}

struct CompiledScriptClosure {
    admitted_digest: String,
    chunk_name: String,
    admitted_bytes: usize,
    function: RegistryKey,
    environment: RegistryKey,
}

#[derive(Default)]
struct Lifecycle {
    world: Option<Weak<RefCell<world::World>>>,
    catalog: Map<String, Json>,
    local_sources: Map<String, Json>,
    builtins: Vec<Json>,
    builtin_sources: HashMap<String, Json>,
    modules: HashMap<String, ModuleEntry>,
    scripts: HashMap<String, ScriptEntry>,
    self_destroyed: HashSet<String>,
    compiled_script_closures: HashMap<String, CompiledScriptClosure>,
    origins: HashMap<String, String>,
    actor_id: Option<String>,
    initial_ready: bool,
    templates_ready: bool,
    bootstrap_errors: Vec<String>,
}

fn err(message: impl Into<String>) -> mlua::Error {
    mlua::Error::RuntimeError(message.into())
}

fn value_error_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.to_string_lossy(),
        other => format!("module failed: {other:?}"),
    }
}

fn hex_decode(value: &str) -> Result<Vec<u8>> {
    if value.len() % 2 != 0 {
        return Err(err("invalid bytecode encoding"));
    }
    let mut output = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks_exact(2) {
        let digit = |byte: u8| (byte as char).to_digit(16).map(|v| v as u8);
        let (Some(hi), Some(lo)) = (digit(pair[0]), digit(pair[1])) else {
            return Err(err("invalid bytecode encoding"));
        };
        output.push((hi << 4) | lo);
    }
    Ok(output)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

fn validate_original_bytecode(entry: &Json) -> Result<()> {
    let Some(original) = entry.get("original_bytecode_hex") else {
        return Ok(());
    };
    let normalized = entry
        .get("bytecode_hex")
        .and_then(Json::as_str)
        .ok_or_else(|| err("invalid original bytecode metadata"))?;
    let original = original
        .as_str()
        .ok_or_else(|| err("invalid original bytecode metadata"))?;
    let original_sha = entry
        .get("original_sha256")
        .and_then(Json::as_str)
        .ok_or_else(|| err("invalid original bytecode metadata"))?;
    let normalized_sha = entry
        .get("normalized_sha256")
        .and_then(Json::as_str)
        .ok_or_else(|| err("invalid original bytecode metadata"))?;
    if original.is_empty()
        || original.len() > 256 * 1024
        || normalized.is_empty()
        || normalized.len() > 256 * 1024
    {
        return Err(err("invalid original bytecode size"));
    }
    if sha256_hex(&hex_decode(original)?) != original_sha {
        return Err(err("original bytecode digest mismatch"));
    }
    if sha256_hex(&hex_decode(normalized)?) != normalized_sha {
        return Err(err("normalized bytecode digest mismatch"));
    }
    Ok(())
}

fn validate_catalog(value: &Json) -> Result<Map<String, Json>> {
    crate::runtime_sources::validate_catalog(value).map_err(|e| err(e.to_string()))?;
    let entries = value
        .as_object()
        .ok_or_else(|| err("script catalog must be an object"))?;
    if entries.len() > MAX_CATALOG_ENTRIES
        || serde_json::to_vec(value)
            .map_err(mlua::Error::external)?
            .len()
            > MAX_CATALOG_BYTES
    {
        return Err(err("invalid script source catalog"));
    }
    for entry in entries.values() {
        if !entry.is_object() {
            return Err(err("invalid script code entry"));
        }
        if entry.get("bytecode_provenance").is_some()
            || entry.get("bytecode_compiler_commit").is_some()
        {
            return Err(err(
                "builtin bytecode metadata is reserved for the pinned player script bundle",
            ));
        }
        if entry
            .get("source")
            .and_then(Json::as_str)
            .is_some_and(|s| s.len() > MAX_SOURCE)
        {
            return Err(err("invalid script source"));
        }
        if let Some(bytecode) = entry.get("bytecode_hex") {
            let hex = bytecode
                .as_str()
                .ok_or_else(|| err("invalid script bytecode"))?;
            let verified = entry.get("signature_verified").and_then(Json::as_bool) == Some(true);
            let small = !verified
                && entry.get("provenance").and_then(Json::as_str) == Some("session_small_bytecode")
                && entry.get("admission").and_then(Json::as_str)
                    == Some("pinned_session_rsb_luau13_14_no_trailer_under_256")
                && hex.len() >= 4
                && hex.len() < 512
                && (hex.starts_with("0d03") || hex.starts_with("0e03"));
            if (!verified && !small) || hex.len() > 256 * 1024 || hex.len() % 2 != 0 {
                return Err(err("unverified or oversized script bytecode"));
            }
            let _ = hex_decode(hex)?;
        }
        validate_original_bytecode(entry)?;
    }
    Ok(entries.clone())
}

fn validate_builtin_bundle(value: &Json) -> Result<Vec<Json>> {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| err("pinned builtin bundle root is unavailable"))?;
    let pinned = crate::runtime_sources::load_builtin_player_scripts(repo_root)
        .map_err(|e| err(format!("cannot verify pinned builtin scripts: {e}")))?;
    if &pinned != value {
        return Err(err(
            "builtin player script bundle differs from the pinned local bundle",
        ));
    }
    let entries = value
        .as_array()
        .ok_or_else(|| err("invalid builtin player script bundle"))?;
    if entries.len() > MAX_BUNDLE_ENTRIES
        || serde_json::to_vec(value)
            .map_err(mlua::Error::external)?
            .len()
            > MAX_BUNDLE_BYTES
    {
        return Err(err("invalid builtin player script bundle"));
    }
    for item in entries {
        let path = item
            .get("path")
            .and_then(Json::as_array)
            .ok_or_else(|| err("invalid builtin script path"))?;
        if path.is_empty()
            || path.len() > 16
            || path
                .iter()
                .any(|part| part.as_str().is_none_or(|s| s.is_empty() || s.len() > 128))
        {
            return Err(err("invalid builtin script path"));
        }
        let class = item
            .get("class_name")
            .and_then(Json::as_str)
            .ok_or_else(|| err("invalid builtin script class"))?;
        if !matches!(class, "LocalScript" | "ModuleScript" | "Folder") {
            return Err(err("invalid builtin script class"));
        }
        if class != "Folder"
            && item
                .get("source")
                .and_then(Json::as_str)
                .is_none_or(|s| s.len() > MAX_SOURCE)
        {
            return Err(err("invalid builtin script source"));
        }
        if item.get("bytecode_hex").is_some() {
            let source = item
                .get("source")
                .and_then(Json::as_str)
                .ok_or_else(|| err("invalid pinned builtin bytecode provenance"))?;
            let encoded = item
                .get("bytecode_hex")
                .and_then(Json::as_str)
                .ok_or_else(|| err("invalid pinned builtin bytecode provenance"))?;
            if item.get("bytecode_provenance").and_then(Json::as_str) != Some(BUILTIN_PROVENANCE)
                || item.get("bytecode_compiler_commit").and_then(Json::as_str)
                    != Some(BUILTIN_COMMIT)
                || item.get("bytecode_version").and_then(Json::as_u64) != Some(6)
                || item.get("bytecode_type_version").and_then(Json::as_u64) != Some(3)
                || !item
                    .get("repository_path")
                    .and_then(Json::as_str)
                    .is_some_and(|p| p.starts_with("scripts/PlayerScripts/"))
                || encoded.len() > 1024 * 1024
                || item.get("source_sha256").and_then(Json::as_str)
                    != Some(sha256_hex(source.as_bytes()).as_str())
            {
                return Err(err("invalid pinned builtin bytecode provenance"));
            }
            let bytes = hex_decode(encoded)?;
            if bytes.len() < 2
                || bytes[0] != 6
                || bytes[1] != 3
                || item.get("bytecode_sha256").and_then(Json::as_str)
                    != Some(sha256_hex(&bytes).as_str())
            {
                return Err(err("pinned builtin bytecode hash or header mismatch"));
            }
        }
    }
    Ok(entries.clone())
}

fn validate_builtin_entry(item: &Json) -> Result<()> {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| err("pinned builtin bundle root is unavailable"))?;
    let pinned = crate::runtime_sources::load_builtin_player_scripts(repo_root)
        .map_err(|e| err(format!("cannot verify pinned builtin scripts: {e}")))?;
    let entries = validate_builtin_bundle(&pinned)?;
    if entries.iter().any(|candidate| candidate == item) {
        Ok(())
    } else {
        Err(err("builtin script differs from the pinned local bundle"))
    }
}

fn script_environment(lua: &Lua, instance: Value) -> Result<Table> {
    let env = lua.create_table()?;
    env.set("script", instance)?;
    let metatable = lua.create_table()?;
    let engine: Table = lua.globals().get::<Function>("getrenv")?.call(())?;
    metatable.set("__index", engine)?;
    env.set_metatable(Some(metatable))?;
    Ok(env)
}

fn is_engine_global(name: &str) -> bool {
    matches!(name,
        "_VERSION" | "assert" | "collectgarbage" | "error" | "gcinfo" | "getfenv"
        | "ipairs" | "next" | "pairs" | "pcall" | "print" | "rawequal" | "rawget"
        | "rawlen" | "rawset" | "select" | "setfenv" | "setmetatable" | "tonumber"
        | "tostring" | "type" | "typeof" | "unpack" | "warn" | "xpcall"
        | "bit32" | "buffer" | "coroutine" | "debug" | "math" | "os" | "string" | "table"
        | "utf8" | "vector" | "Axes" | "BrickColor" | "CFrame" | "Color3" | "ColorSequence"
        | "ColorSequenceKeypoint" | "DateTime" | "Enum" | "Faces" | "FloatCurveKey" | "Font" | "Instance"
        | "NumberRange" | "NumberSequence" | "NumberSequenceKeypoint" | "OverlapParams"
        | "PathWaypoint" | "PhysicalProperties" | "Random" | "Ray" | "RaycastParams" | "Rect"
        | "Region3" | "Region3int16" | "RotationCurveKey" | "SharedTable" | "TweenInfo" | "UDim" | "UDim2"
        | "Vector2" | "Vector2int16" | "Vector3" | "Vector3int16"
        | "game" | "workspace" | "settings" | "UserSettings" | "shared"
        | "require" | "task" | "time" | "tick" | "elapsedTime" | "wait" | "spawn" | "delay")
}

fn engine_debug_table(lua: &Lua, source: Table) -> Result<Table> {
    let out = lua.create_table()?;
    for pair in source.pairs::<Value, Value>() {
        let (key, value) = pair?;
        let Some(name) = key.as_string().and_then(|s| s.to_str().ok()) else {
            continue;
        };
        // Scripts see the regular Luau debug surface. Host inspection tools,
        // stack mutation and function internals stay in the executor VM.
        if matches!(name.as_ref(), "info" | "traceback" | "profilebegin" | "profileend" | "resetmemorycategory" | "setmemorycategory") {
            out.raw_set(key.clone(), value.clone())?;
        }
    }
    Ok(out)
}

fn engine_environment(lua: &Lua) -> Result<Table> {
    let globals = lua.globals();
    let engine = lua.create_table()?;
    for pair in globals.pairs::<Value, Value>() {
        let (key, value) = pair?;
        let Some(name) = key.as_string().and_then(|s| s.to_str().ok()) else {
            continue;
        };
        if !is_engine_global(name.as_ref()) || name == "_G" || matches!(name.as_ref(), "game" | "workspace") {
            continue;
        }
        if name == "debug" {
            if let Value::Table(debug) = value {
                engine.raw_set(key.clone(), engine_debug_table(lua, debug)?)?;
            }
        } else {
            engine.raw_set(key.clone(), value.clone())?;
        }
    }
    // Roblox gives game scripts a shared `_G` table separate from the engine
    // globals table and from the executor's `_G`.
    engine.raw_set("_G", lua.create_table()?)?;
    // DataModel and Workspace can arrive after the Lua host is initialized.
    // Keep these service globals live without falling through to executor APIs.
    let live_globals = globals;
    let metatable = lua.create_table()?;
    metatable.set(
        "__index",
        lua.create_function(move |_, (_table, key): (Table, Value)| {
            let Some(name) = key.as_string() else {
                return Ok(Value::Nil);
            };
            let name = name.to_str()?;
            if matches!(name.as_ref(), "game" | "workspace") {
                live_globals.raw_get(name.as_ref())
            } else {
                Ok(Value::Nil)
            }
        })?,
    )?;
    engine.set_metatable(Some(metatable))?;
    Ok(engine)
}

fn source_item(lifecycle: &Lifecycle, ctx: &WorldRef, id: &str) -> Result<Json> {
    if let Some(item) = lifecycle.local_sources.get(id).filter(|item| has_script_code(item)) {
        return Ok(item.clone());
    }
    if let Some(node) = ctx.borrow().nodes.get(id) {
        if let Some(source) = node.local_overrides.get("Source").and_then(Json::as_str) {
            return Ok(json!({"source":source,"authored_override":true}));
        }
        // A received Source property is the downloaded script text for this
        // node; use it whether or not the node is server-owned. Locally
        // created objects additionally expose their authored default.
        if !lifecycle.origins.contains_key(id) {
            // Source can live on the projected record for received scripts.
            let received_source = node
                .projected
                .as_ref()
                .and_then(|p| p.property("Source").ok().flatten())
                .and_then(|v| v.as_str().map(str::to_owned))
                .filter(|source| !source.is_empty());
            let source = node
                .properties
                .get("Source")
                .and_then(Json::as_str)
                .filter(|source| !source.is_empty())
                .map(str::to_owned)
                .or(received_source);
            if let Some(source) = source {
                return Ok(json!({"source":source}));
            }
        }
    }
    if let Some(item) = lifecycle.builtin_sources.get(id).filter(|item| has_script_code(item)) {
        return Ok(item.clone());
    }
    if let Some(item) = lifecycle.catalog.get(id).filter(|item| has_script_code(item)) {
        return Ok(item.clone());
    }
    let mut origin = lifecycle.origins.get(id).cloned();
    for _ in 0..64 {
        let Some(current) = origin else {
            break;
        };
        if let Some(item) = lifecycle
            .local_sources
            .get(&current)
            .filter(|item| has_script_code(item))
            .or_else(|| lifecycle.catalog.get(&current).filter(|item| has_script_code(item)))
            .or_else(|| lifecycle.builtin_sources.get(&current).filter(|item| has_script_code(item)))
        {
            return Ok(item.clone());
        }
        origin = lifecycle.origins.get(&current).cloned();
    }
    Err(err("code_unavailable"))
}

fn has_script_code(item: &Json) -> bool {
    item.get("source").and_then(Json::as_str).is_some()
        || item.get("bytecode_hex").and_then(Json::as_str).is_some_and(|hex| !hex.is_empty())
}

/// Preserve the source lineage for an Instance:Clone() result without changing
/// whether the original script was admitted by this VM.
pub(super) fn record_clone(lua: &Lua, source_id: &str, clone_id: &str) -> Result<()> {
    let Some(state) = lua
        .app_data_ref::<Rc<RefCell<Lifecycle>>>()
        .map(|state| state.clone())
    else {
        return Ok(());
    };
    state
        .borrow_mut()
        .origins
        .insert(clone_id.to_owned(), source_id.to_owned());
    Ok(())
}

/// Received `Source` can be raw Luau bytecode rather than text; detect the
/// chunk signature so it is loaded instead of fed to the text compiler.
fn looks_like_bytecode(bytes: &[u8]) -> bool {
    bytes.len() > 4 && (4..=14).contains(&bytes[0]) && bytes[1] <= 3
}

fn compile_item(lua: &Lua, name: &str, item: &Json) -> Result<Function> {
    if let Some(source) = item.get("source").and_then(Json::as_str) {
        if item.get("bytecode_provenance").is_none() && looks_like_bytecode(source.as_bytes()) {
            return super::integrity::load_bytecode(lua, name, &[], source.as_bytes());
        }
        if item.get("bytecode_provenance").is_some() {
            validate_builtin_entry(item)?;
            let bytes = hex_decode(
                item.get("bytecode_hex")
                    .and_then(Json::as_str)
                    .ok_or_else(|| err("pinned builtin bytecode missing"))?,
            )?;
            return super::integrity::load_bytecode(lua, name, source.as_bytes(), &bytes);
        }
        return super::integrity::load_source(lua, name, source);
    }
    let encoded = item
        .get("bytecode_hex")
        .and_then(Json::as_str)
        .ok_or_else(|| {
            err(item
                .get("reason")
                .and_then(Json::as_str)
                .unwrap_or("code_unavailable"))
        })?;
    let bytecode = hex_decode(encoded)?;
    super::integrity::load_bytecode(lua, name, &[], &bytecode)
}

fn compile_for_instance(
    lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    instance: Value,
    state: &Rc<RefCell<Lifecycle>>,
) -> Result<(Function, Table)> {
    let class = ctx
        .borrow()
        .nodes
        .get(id)
        .map(|node| node.class_name.clone())
        .ok_or_else(|| err("script is not in this world"))?;
    if !matches!(class.as_str(), "LocalScript" | "Script" | "ModuleScript") {
        return Err(err("script Instance required"));
    }
    let item = source_item(&state.borrow(), ctx, id)?;
    let chunk_name = {
        let world = ctx.borrow();
        format!("={}", world::instance_full_name(&world, id)?)
    };
    let admitted_data = serde_json::to_vec(&item).map_err(mlua::Error::external)?;
    let admitted_digest = sha256_hex(&admitted_data);
    {
        let lifecycle = state.borrow();
        let running = lifecycle.scripts.get(id).is_some_and(|entry| entry.active);
        let required = lifecycle.modules.get(id).is_some_and(|entry| {
            matches!(entry.state, ModuleState::Loaded | ModuleState::Failed)
        });
        if let Some(cached) = lifecycle.compiled_script_closures.get(id) {
            // An already-started script or a completed require keeps the exact
            // admitted Proto it ran. Otherwise a digest change invalidates the
            // compile cache before the next start.
            if (cached.admitted_digest == admitted_digest && cached.chunk_name == chunk_name)
                || running
                || required
            {
                let function = lua.registry_value(&cached.function)?;
                let environment = lua.registry_value(&cached.environment)?;
                return Ok((function, environment));
            }
        }
    }
    let (cached_bytes, current_bytes, current_count) = {
        let lifecycle = state.borrow();
        let cached = lifecycle
            .compiled_script_closures
            .get(id)
            .map_or(0, |entry| entry.admitted_bytes);
        let total = lifecycle
            .compiled_script_closures
            .values()
            .map(|entry| entry.admitted_bytes)
            .sum::<usize>();
        (cached, total, lifecycle.compiled_script_closures.len())
    };
    let new_count = current_count + usize::from(cached_bytes == 0);
    let new_bytes = current_bytes.saturating_sub(cached_bytes).saturating_add(admitted_data.len());
    if new_count > MAX_COMPILED_SCRIPT_CLOSURES || new_bytes > MAX_COMPILED_SCRIPT_BYTES {
        return Err(err("compiled script closure cache limit exceeded"));
    }

    let env = script_environment(lua, instance)?;
    let function = compile_item(lua, &chunk_name, &item)?;
    if !function.set_environment(env.clone())? {
        return Err(err("could not set script environment"));
    }
    let function_key = lua.create_registry_value(function.clone())?;
    let environment_key = lua.create_registry_value(env.clone())?;
    state.borrow_mut().compiled_script_closures.insert(
        id.to_owned(),
        CompiledScriptClosure {
            admitted_digest,
            chunk_name,
            admitted_bytes: admitted_data.len(),
            function: function_key,
            environment: environment_key,
        },
    );
    Ok((function, env))
}

/// Report only whether a live Actor VM owns this running script. Environment
/// tables stay inside their originating Lua state and are never returned to
/// the caller's VM.
fn is_running_in_actor_vm(lua: &Lua, id: &str) -> Result<bool> {
    if !super::actors::has_live_vms(lua) {
        return Ok(false);
    }
    let mut found = false;
    super::actors::for_each_actor(lua, |_, actor_lua, _, _| {
        if found {
            return Ok(());
        }
        let Some(actor_state) = actor_lua.app_data_ref::<Rc<RefCell<Lifecycle>>>() else {
            return Ok(());
        };
        let actor_state = actor_state.borrow();
        let script_running = actor_state.scripts.get(id).is_some_and(|entry| {
            entry.active
                && entry.thread.as_ref().is_some_and(|thread| {
                    thread.status() != ThreadStatus::Finished
                        && thread.status() != ThreadStatus::Error
                })
        });
        let module_running = actor_state.modules.get(id).is_some_and(|entry| {
            entry.state == ModuleState::Loading && entry.owner_thread.is_some()
        });
        found = script_running || module_running;
        Ok(())
    })?;
    Ok(found)
}

fn property(node: &world::Node, name: &str) -> Option<Json> {
    world::node_property(node, name)
}

fn ref_id(value: Option<Json>) -> Option<String> {
    let value = value?;
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.get("$ref").and_then(Json::as_str).map(str::to_owned))
}

fn enum_number(value: Option<Json>) -> Option<i64> {
    let value = value?;
    value.as_i64().or_else(|| {
        value
            .get("$enum")
            .and_then(|e| e.get("value"))
            .and_then(Json::as_i64)
    })
}

fn has_ancestor_class(world: &world::World, id: &str, class: &str) -> bool {
    let mut cursor = world.nodes.get(id).and_then(|n| n.parent.clone());
    let mut seen = HashSet::new();
    while let Some(current) = cursor {
        if !seen.insert(current.clone()) {
            break;
        }
        let Some(node) = world.nodes.get(&current) else {
            break;
        };
        if node.class_name == class {
            return true;
        }
        cursor = node.parent.clone();
    }
    false
}

fn in_game(world: &world::World, id: &str) -> bool {
    has_ancestor_class(world, id, "DataModel")
        || world
            .nodes
            .get(id)
            .is_some_and(|node| node.class_name == "DataModel")
}

fn local_player_and_character(ctx: &WorldRef) -> (Option<String>, Option<String>) {
    let world = ctx.borrow();
    let player = world
        .snapshot
        .get("local_player")
        .and_then(Json::as_str)
        .filter(|id| world.nodes.get(*id).is_some_and(|node| {
            !node.destroyed && node.class_name == "Player"
        }))
        .map(str::to_owned);
    let character = player
        .as_ref()
        .and_then(|id| world.nodes.get(id))
        .and_then(|n| ref_id(property(n, "Character")));
    (player, character)
}

fn eligible(
    ctx: &WorldRef,
    actor_id: Option<&str>,
    id: &str,
    player: Option<&str>,
    character: Option<&str>,
) -> (bool, &'static str) {
    let world = ctx.borrow();
    let Some(node) = world.nodes.get(id) else {
        return (false, "missing_instance");
    };
    if node.destroyed {
        return (false, "destroyed");
    }
    if !in_game(&world, id) {
        return (false, "outside_game");
    }
    if property(node, "Disabled").and_then(|v| v.as_bool()) == Some(true)
        || property(node, "Enabled").and_then(|v| v.as_bool()) == Some(false)
    {
        return (false, "disabled");
    }
    if let Some(actor_id) = actor_id {
        if node.class_name != "LocalScript"
            && !(node.class_name == "Script"
                && enum_number(property(node, "RunContext")) == Some(2))
        {
            return (false, "actor_class_or_runcontext");
        }
        let mut cursor = node.parent.clone();
        while let Some(current) = cursor {
            let Some(parent) = world.nodes.get(&current) else {
                break;
            };
            if parent.class_name == "Actor" {
                return if current == actor_id {
                    (true, "eligible")
                } else {
                    (false, "different_actor")
                };
            }
            cursor = parent.parent.clone();
        }
        return (false, "no_actor_ancestor");
    }
    if has_ancestor_class(&world, id, "Actor") {
        return (false, "actor_owned");
    }
    if node.class_name == "Script" {
        return if enum_number(property(node, "RunContext")) == Some(2) {
            (true, "eligible")
        } else {
            (false, "script_runcontext_not_client")
        };
    }
    if node.class_name != "LocalScript" {
        return (false, "unsupported_class");
    }
    let mut cursor = node.parent.clone();
    while let Some(parent_id) = cursor {
        let Some(parent_node) = world.nodes.get(&parent_id) else {
            break;
        };
        if parent_node.class_name == "ReplicatedFirst" || Some(parent_id.as_str()) == character {
            return (true, "eligible");
        }
        if Some(parent_id.as_str()) == player && matches!(node.class_name.as_str(), "LocalScript") {
            return (false, "local_script_outside_client_container");
        }
        if parent_node.parent.as_deref() == player
            && matches!(
                parent_node.class_name.as_str(),
                "PlayerScripts" | "PlayerGui" | "Backpack"
            )
        {
            return (true, "eligible");
        }
        cursor = parent_node.parent.clone();
    }
    (false, "local_script_outside_client_container")
}

fn cancel_thread(lua: &Lua, thread: &Thread) -> Result<()> {
    if !thread.is_resumable() {
        return Ok(());
    }
    let task: Table = lua.globals().get("task")?;
    let cancel: Function = task.get("cancel")?;
    cancel.call::<()>(thread.clone())
}

/// Apply the script-owner cleanup performed by Instance:Destroy(). The
/// currently executing owner coroutine survives its own Destroy; all other
/// queued work from the destroyed script owners is closed immediately.
pub(super) fn destroy_script_instances(lua: &Lua, ids: &[String]) -> Result<()> {
    let self_destroyed = scheduler::destroy_script_owners(lua, ids)?;
    if let Some(state) = lua
        .app_data_ref::<Rc<RefCell<Lifecycle>>>()
        .map(|state| state.clone())
    {
        state.borrow_mut().self_destroyed.extend(self_destroyed);
    }
    Ok(())
}

fn status_json(state: &Lifecycle) -> Json {
    let mut result = json!({"local_scripts":0,"running":0,"completed":0,"failed":0,"blocked":0,"stopped":0,
        "modules_loaded":0,"modules_loading":0,"modules_failed":0,"bootstrap_errors":state.bootstrap_errors.len(),"details":[]});
    for entry in state.scripts.values() {
        if let Some(count) = result
            .get_mut(&entry.state)
            .and_then(|value| value.as_u64())
        {
            result[&entry.state] = json!(count + 1);
        }
        result["local_scripts"] = json!(result["local_scripts"].as_u64().unwrap_or(0) + 1);
        if let Some(details) = result["details"].as_array_mut() {
            if details.len() < 128 {
                details.push(json!({"id":entry.id,"state":entry.state,"error":entry.error}));
            }
        }
    }
    for (id, entry) in &state.modules {
        let key = match entry.state {
            ModuleState::Loading => "modules_loading",
            ModuleState::Loaded => "modules_loaded",
            ModuleState::Failed => "modules_failed",
        };
        result[key] = json!(result[key].as_u64().unwrap_or(0) + 1);
        if matches!(entry.state, ModuleState::Failed) {
            if let Some(details) = result["details"].as_array_mut() {
                if details.len() < 128 {
                    details
                        .push(json!({"module":id,"state":"failed","error":entry.error.as_deref()}));
                }
            }
        }
    }
    result
}

/// Install per-VM lifecycle state and helpers. Source metadata is validated at
/// admission; execution still goes through `integrity::load_source/bytecode`.
pub fn install(lua: &Lua, ctx: &WorldRef) -> Result<()> {
    let snapshot = ctx.borrow().snapshot.clone();
    let catalog = match snapshot.get("script_sources") {
        Some(value) => validate_catalog(value)?,
        None => Map::new(),
    };
    let builtins = match snapshot.get("builtin_player_scripts") {
        Some(value) => validate_builtin_bundle(value)?,
        None => Vec::new(),
    };
    let mut state = Lifecycle {
        world: Some(Rc::downgrade(ctx)),
        catalog,
        builtins,
        initial_ready: snapshot
            .get("initial_replication_complete")
            .and_then(Json::as_bool)
            .unwrap_or(true),
        actor_id: snapshot
            .get("actor_id")
            .and_then(Json::as_str)
            .map(str::to_owned),
        ..Lifecycle::default()
    };
    if let Some(id) = super::actors::actor_id(lua) {
        state.actor_id = Some(id);
    }
    let state = Rc::new(RefCell::new(state));
    lua.set_app_data(state.clone());
    let globals = lua.globals();
    let env = globals.clone();
    globals.set(
        "getrenv",
        lua.create_function(move |_, ()| Ok(env.clone()))?,
    )?;

    let weak_world = Rc::downgrade(ctx);
    let require_state = state.clone();
    let begin_require = lua.create_function(move |lua, module: Value| {
            if !world::instance_belongs_to(
                &module,
                &weak_world
                    .upgrade()
                    .ok_or_else(|| err("world has been released"))?,
            ) {
                return Err(err("require expects a ModuleScript Instance from this VM"));
            }
            let id = world::instance_id(&module)
                .ok_or_else(|| err("require expects a ModuleScript Instance"))?;
            let ctx = weak_world
                .upgrade()
                .ok_or_else(|| err("world has been released"))?;
            if ctx
                .borrow()
                .nodes
                .get(&id)
                .is_none_or(|n| n.class_name != "ModuleScript")
            {
                return Err(err("require expects a ModuleScript Instance"));
            }
            let caller = lua.current_thread();
            let caller_key = caller.to_pointer() as usize;
            let existing = {
                let mut state = require_state.borrow_mut();
                let owner_module = state
                    .modules
                    .iter()
                    .find_map(|(module_id, entry)| (entry.owner_thread == Some(caller_key)).then(|| module_id.clone()));
                if let Some(module_state) = state.modules.get(&id).map(|entry| entry.state) {
                    match module_state {
                        ModuleState::Loaded => {
                            let entry = state.modules.get(&id).ok_or_else(|| err("module cache entry is incomplete"))?;
                            Some((true, Some(lua.registry_value::<Value>(entry.value.as_ref().ok_or_else(|| err("module cache entry is incomplete"))?)?), None))
                        }
                        ModuleState::Failed => {
                            let entry = state.modules.get(&id).ok_or_else(|| err("module cache entry is incomplete"))?;
                            Some((true, None, Some(entry.error.clone().unwrap_or_else(|| "module failed".into()))))
                        }
                        ModuleState::Loading => {
                            if state.modules.get(&id).and_then(|entry| entry.owner_thread) == Some(caller_key) {
                                return Err(err("circular ModuleScript require"));
                            }
                            if let Some(owner_id) = owner_module.as_ref() {
                                let mut cursor = Some(id.as_str());
                                let mut seen = HashSet::new();
                                while let Some(current) = cursor {
                                    if current == owner_id {
                                        return Err(err("circular ModuleScript require"));
                                    }
                                    if !seen.insert(current.to_owned()) {
                                        break;
                                    }
                                    cursor = state.modules.get(current).and_then(|candidate| candidate.waiting_for.as_deref());
                                }
                                if let Some(owner) = state.modules.get_mut(owner_id) {
                                    owner.waiting_for = Some(id.clone());
                                }
                            }
                            if let Some(entry) = state.modules.get_mut(&id) {
                                entry.waiters.retain(|thread| scheduler::is_waiting(lua, thread));
                                entry.waiters.push(caller.clone());
                            }
                            None
                        }
                    }
                } else {
                    state.modules.insert(id.clone(), ModuleEntry {
                        state: ModuleState::Loading,
                        value: None,
                        environment: None,
                        error: None,
                        owner_thread: None,
                        // The coroutine that first requests the module is a
                        // waiter too; completion resumes it with the cached
                        // result just like later concurrent callers.
                        waiters: vec![caller.clone()],
                        waiting_for: None,
                    });
                    if let Some(owner_id) = owner_module {
                        if let Some(owner) = state.modules.get_mut(&owner_id) {
                            owner.waiting_for = Some(id.clone());
                        }
                    }
                    Some((false, None, None))
                }
            };
            if let Some((true, value, failure)) = existing {
                return Ok((true, value.unwrap_or(Value::Nil), failure));
            }
            if existing.is_some() {
                let (function, environment) = match compile_for_instance(lua, &ctx, &id, module.clone(), &require_state) {
                    Ok(compiled) => compiled,
                    Err(error) => {
                        if let Some(entry) = require_state.borrow_mut().modules.get_mut(&id) {
                            entry.state = ModuleState::Failed;
                            entry.error = Some(error.to_string());
                            entry.waiters.clear();
                        }
                        return Ok((true, Value::Nil, Some(error.to_string())));
                    }
                };
                let environment_key = lua.create_registry_value(environment)?;
                let environment_key = Rc::new(RefCell::new(Some(environment_key)));
                let done_environment = environment_key.clone();
                let state_on_done = require_state.clone();
                let module_id = id.clone();
                let done = lua.create_function(move |lua, result: MultiValue| {
                    let Some(environment_key) = done_environment.borrow_mut().take() else {
                        return Ok(());
                    };
                    let mut values = result.into_iter();
                    let succeeded = matches!(values.next(), Some(Value::Boolean(true)));
                    let returned: Vec<Value> = values.collect();
                    let (resume, response) = {
                        let mut state = state_on_done.borrow_mut();
                        let mut response = MultiValue::new();
                        let success = succeeded && returned.len() == 1;
                        let waiters = {
                            let Some(entry) = state.modules.get_mut(&module_id) else { return Ok(()); };
                            if success {
                                let value = returned.into_iter().next().unwrap_or(Value::Nil);
                                entry.value = Some(lua.create_registry_value(value.clone())?);
                                entry.state = ModuleState::Loaded;
                                entry.error = None;
                                response.push_back(Value::Boolean(true));
                                response.push_back(value);
                            } else {
                                let message = if succeeded {
                                    "Module code must return exactly one value".to_owned()
                                } else {
                                    returned.first().map(value_error_text).unwrap_or_else(|| "module failed".into())
                                };
                                entry.state = ModuleState::Failed;
                                entry.error = Some(message.clone());
                                response.push_back(Value::Boolean(false));
                                response.push_back(Value::String(lua.create_string(message)?));
                            }
                            entry.environment = Some(environment_key);
                            entry.owner_thread = None;
                            std::mem::take(&mut entry.waiters)
                        };
                        for candidate in state.modules.values_mut() {
                            if candidate.waiting_for.as_deref() == Some(module_id.as_str()) {
                                candidate.waiting_for = None;
                            }
                        }
                        (waiters, response)
                    };
                    for thread in resume {
                        if scheduler::is_waiting(lua, &thread) {
                            scheduler::resume(lua, thread, response.clone())?;
                        }
                    }
                    Ok(())
                })?;
                let runner: Function = lua.load("return function(run, done) local result=table.pack(pcall(run)); done(table.unpack(result,1,result.n)) end").eval()?;
                let mut args = MultiValue::new();
                args.push_back(Value::Function(function));
                args.push_back(Value::Function(done));
                match scheduler::enqueue_module(lua, runner, args, &id, module.clone()) {
                    Ok(thread) => {
                        if let Some(entry) = require_state.borrow_mut().modules.get_mut(&id) {
                            entry.owner_thread = Some(thread.to_pointer() as usize);
                        }
                    }
                    Err(error) => {
                        if let Some(entry) = require_state.borrow_mut().modules.get_mut(&id) {
                            entry.state = ModuleState::Failed;
                            entry.error = Some(error.to_string());
                            entry.waiters.clear();
                        }
                        return Ok((true, Value::Nil, Some(error.to_string())));
                    }
                }
            }
            scheduler::park(lua, &caller)?;
            Ok((false, Value::Nil, None))
        })?;
    let require_factory: Function = lua.load(
        "return function(begin) return function(module)\n\
            if not coroutine.isyieldable() then error('require must run in a yieldable coroutine', 2) end\n\
            local ready, value, failure = begin(module)\n\
            if ready then if failure ~= nil then error(failure, 2) end; return value end\n\
            local result = table.pack(coroutine.yield())\n\
            if not result[1] then error(result[2] or 'ModuleScript failed', 2) end\n\
            return result[2]\n\
        end end"
    ).eval()?;
    globals.set("require", require_factory.call::<Function>(begin_require)?)?;

    let weak_world = Rc::downgrade(ctx);
    let get_closure_state = state.clone();
    globals.set(
        "getscriptclosure",
        lua.create_function(move |lua, value: Value| -> Result<Value> {
            let ctx = weak_world
                .upgrade()
                .ok_or_else(|| err("world has been released"))?;
            if !world::instance_belongs_to(&value, &ctx) {
                return Err(err("getscriptclosure expects an Instance from this VM"));
            }
            let id = world::instance_id(&value).ok_or_else(|| err("script Instance required"))?;
            match compile_for_instance(lua, &ctx, &id, value, &get_closure_state) {
                Ok((function, _)) => {
                    // Clone the closure through the VM API, preserving its
                    // admitted Proto and environment without consulting a
                    // mutable executor-global function.
                    let clone: Function = unsafe {
                        lua.exec_raw((function,), |state| {
                            let slot = mlua::ffi::lua_gettop(state);
                            mlua::ffi::lua_clonefunction(state, slot);
                            mlua::ffi::lua_getfenv(state, slot);
                            mlua::ffi::lua_setfenv(state, -2);
                            mlua::ffi::lua_remove(state, slot);
                        })?
                    };
                    Ok(Value::Function(clone))
                }
                Err(mlua::Error::RuntimeError(message)) if message == "code_unavailable" => {
                    Ok(Value::Nil)
                }
                Err(error) => Err(error),
            }
        })?,
    )?;
    globals.set(
        "getscriptfunction",
        globals.get::<Value>("getscriptclosure")?,
    )?;

    let weak_world = Rc::downgrade(ctx);
    let module_state = state.clone();
    globals.set(
        "getloadedmodules",
        lua.create_function(move |lua, ()| {
            let ids: Vec<String> = module_state
                .borrow()
                .modules
                .iter()
                .filter(|(_, e)| e.state == ModuleState::Loaded)
                .map(|(id, _)| id.clone())
                .collect();
            let ctx = weak_world
                .upgrade()
                .ok_or_else(|| err("world has been released"))?;
            let out = lua.create_table_with_capacity(ids.len(), 0)?;
            for (i, id) in ids.into_iter().enumerate() {
                out.raw_set(i + 1, world::instance(lua, &ctx, &id)?)?;
            }
            Ok(out)
        })?,
    )?;
    let weak_world = Rc::downgrade(ctx);
    let running_state = state.clone();
    globals.set(
        "getrunningscripts",
        lua.create_function(move |lua, ()| {
            let mut ids: HashSet<String> = running_state
                .borrow()
                .modules
                .iter()
                .filter(|(_, e)| e.state == ModuleState::Loading)
                .map(|(id, _)| id.clone())
                .collect();
            ids.extend(
                running_state
                    .borrow()
                    .scripts
                    .iter()
                    .filter(|(_, e)| {
                        e.active
                            && e.thread.as_ref().is_some_and(|t| {
                                t.status() != ThreadStatus::Finished
                                    && t.status() != ThreadStatus::Error
                            })
                    })
                    .map(|(id, _)| id.clone()),
            );
            // getrunningscripts is scoped to this VM's global state. Include
            // its actual active scheduler-owned threads and deduplicate them
            // against the lifecycle's running-entry map.
            ids.extend(super::script_threads::running_script_ids(lua)?);
            let ctx = weak_world
                .upgrade()
                .ok_or_else(|| err("world has been released"))?;
            ids.retain(|id| ctx.borrow().nodes.contains_key(id));
            let out = lua.create_table_with_capacity(ids.len(), 0)?;
            for (i, id) in ids.into_iter().enumerate() {
                out.raw_set(i + 1, world::instance(lua, &ctx, &id)?)?;
            }
            Ok(out)
        })?,
    )?;
    let weak_world = Rc::downgrade(ctx);
    let state_for_env = state.clone();
    globals.set(
        "getsenv",
        lua.create_function(move |lua, value: Value| {
            let ctx = weak_world
                .upgrade()
                .ok_or_else(|| err("world has been released"))?;
            if !world::instance_belongs_to(&value, &ctx) {
                return Err(err("script Instance required"));
            }
            let id = world::instance_id(&value).ok_or_else(|| err("script Instance required"))?;
            if !ctx.borrow().nodes.get(&id).is_some_and(|node| {
                !node.destroyed
                    && matches!(node.class_name.as_str(), "Script" | "LocalScript" | "ModuleScript")
            }) {
                return Err(err("script Instance required"));
            }
            let state = state_for_env.borrow();
            if let Some(entry) = state.modules.get(&id).and_then(|e| e.environment.as_ref()) {
                return lua.registry_value::<Value>(entry);
            }
            if let Some(entry) = state.scripts.get(&id) {
                return lua.registry_value(&entry.environment);
            }
            drop(state);
            if is_running_in_actor_vm(lua, &id)? {
                return Ok(Value::Nil);
            }
            Err(err("script has no instantiated environment"))
        })?,
    )?;

    let weak_world = Rc::downgrade(ctx);
    let register_state = state.clone();
    let register = lua.create_function(move |_, (instance, source): (Value, String)| {
        let ctx = weak_world
            .upgrade()
            .ok_or_else(|| err("world has been released"))?;
        if !world::instance_belongs_to(&instance, &ctx) {
            return Err(err("script Instance required"));
        }
        if source.len() > MAX_SOURCE {
            return Err(err("script source exceeds 128 KiB"));
        }
        let id = world::instance_id(&instance).ok_or_else(|| err("script Instance required"))?;
        if ctx.borrow().nodes.get(&id).is_none_or(|n| {
            !matches!(
                n.class_name.as_str(),
                "LocalScript" | "ModuleScript" | "Script"
            )
        }) {
            return Err(err("script Instance required"));
        }
        let mut state = register_state.borrow_mut();
        let mut proposed = state.local_sources.clone();
        proposed.insert(
            id.clone(),
            json!({"source":source,"authored_override":true}),
        );
        if serde_json::to_vec(&Json::Object(proposed.clone()))
            .map_err(mlua::Error::external)?
            .len()
            > MAX_LOCAL_SOURCES_BYTES
        {
            return Err(err("local source catalog exceeds 4 MiB"));
        }
        state.local_sources = proposed;
        Ok(())
    })?;
    let client: Table = globals.get("client").unwrap_or(lua.create_table()?);
    client.set("registerScriptSource", register)?;
    let status_state = state.clone();
    client.set(
        "scriptStatus",
        lua.create_function(move |lua, ()| {
            super::values::from_json(lua, &status_json(&status_state.borrow()))
        })?,
    )?;
    globals.set("client", client)?;
    let engine = engine_environment(lua)?;
    globals.set(
        "getrenv",
        lua.create_function(move |_, ()| Ok(engine.clone()))?,
    )?;
    Ok(())
}

/// Returns script text only when the VM's existing source-admission rules
/// already make it available (authored, local, pinned, or explicitly cataloged).
/// Callers should not use this to turn bytecode-only or protected scripts into
/// readable source.
pub(super) fn script_code(lua: &Lua, ctx: &WorldRef, value: Value) -> Result<Json> {
    if !world::instance_belongs_to(&value, ctx) {
        return Err(err("script Instance required"));
    }
    let id = world::instance_id(&value).ok_or_else(|| err("script Instance required"))?;
    let class = ctx
        .borrow()
        .nodes
        .get(&id)
        .map(|node| node.class_name.clone())
        .ok_or_else(|| err("script Instance required"))?;
    if !matches!(class.as_str(), "Script" | "LocalScript" | "ModuleScript") {
        return Err(err("script Instance required"));
    }
    let state = lua
        .app_data_ref::<Rc<RefCell<Lifecycle>>>()
        .ok_or_else(|| err("script lifecycle is not installed"))?
        .clone();
    let borrowed = state.borrow();
    let result = source_item(&borrowed, ctx, &id);
    drop(borrowed);
    result
}

/// Return the recorded source-instance ID for a client-side clone. This is
/// diagnostic metadata only; callers must not use it to claim server identity.
pub(super) fn replication_origin_id(lua: &Lua, id: &str) -> Option<String> {
    lua.app_data_ref::<Rc<RefCell<Lifecycle>>>()
        .and_then(|state| state.borrow().origins.get(id).cloned())
}

/// Set the actor scope explicitly when constructing an Actor-owned Luau VM.
pub fn set_actor_id(lua: &Lua, actor_id: Option<String>) -> Result<()> {
    let state = lua
        .app_data_ref::<Rc<RefCell<Lifecycle>>>()
        .ok_or_else(|| err("script lifecycle is not installed"))?
        .clone();
    state.borrow_mut().actor_id = actor_id;
    Ok(())
}

fn find_named_child(ctx: &WorldRef, parent: &str, name: &str) -> Option<String> {
    world::children(ctx, parent).into_iter().find(|id| {
        ctx.borrow()
            .nodes
            .get(id)
            .is_some_and(|node| node.name == name)
    })
}

fn map_clone_origins(
    ctx: &WorldRef,
    state: &mut Lifecycle,
    source: &str,
    clone: &str,
    depth: usize,
) {
    if depth > 128 {
        return;
    }
    state.origins.insert(clone.to_owned(), source.to_owned());
    let source_children = world::children(ctx, source);
    let clone_children = world::children(ctx, clone);
    for (a, b) in source_children.iter().zip(clone_children.iter()) {
        map_clone_origins(ctx, state, a, b, depth + 1);
    }
}

fn mirror_children(
    lua: &Lua,
    ctx: &WorldRef,
    state: &Rc<RefCell<Lifecycle>>,
    source: Option<String>,
    target: Option<String>,
) -> Result<()> {
    let (Some(source), Some(target)) = (source, target) else {
        return Ok(());
    };
    let children = world::children(ctx, &source);
    for child in children {
        let name = ctx
            .borrow()
            .nodes
            .get(&child)
            .map(|n| n.name.clone())
            .unwrap_or_default();
        if find_named_child(ctx, &target, &name).is_some() {
            continue;
        }
        match world::clone_instance(lua, ctx, &child) {
            Ok(Value::UserData(clone)) => {
                let clone_value = Value::UserData(clone);
                let clone_id = world::instance_id(&clone_value)
                    .ok_or_else(|| err("cloned template is not an Instance"))?;
                world::set_property(lua, ctx, &clone_id, "Parent", json!({"$ref":target}))?;
                map_clone_origins(ctx, &mut state.borrow_mut(), &child, &clone_id, 0);
            }
            Ok(Value::Nil) => {}
            Ok(_) => return Err(err("Clone returned a non-Instance value")),
            Err(e) => state
                .borrow_mut()
                .bootstrap_errors
                .push(format!("template {name}: {e}")),
        }
    }
    Ok(())
}

fn install_builtins(
    lua: &Lua,
    ctx: &WorldRef,
    state: &Rc<RefCell<Lifecycle>>,
    target: &str,
) -> Result<()> {
    let entries = state.borrow().builtins.clone();
    let overridden: HashSet<String> = world::children(ctx, target)
        .into_iter()
        .filter_map(|id| ctx.borrow().nodes.get(&id).map(|n| n.name.clone()))
        .collect();
    let mut created: HashMap<String, String> = HashMap::new();
    for entry in entries {
        let path: Vec<String> = entry["path"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap().to_owned())
            .collect();
        if overridden.contains(&path[0]) {
            continue;
        }
        for index in 0..path.len() {
            let key = path[..=index].join("/");
            if created.contains_key(&key) {
                continue;
            }
            let parent = if index == 0 {
                target.to_owned()
            } else {
                created
                    .get(&path[..index].join("/"))
                    .cloned()
                    .ok_or_else(|| err("pinned builtin script parent is missing"))?
            };
            let class = if index + 1 == path.len() {
                entry["class_name"].as_str().unwrap()
            } else {
                "Folder"
            };
            let value = world::create_instance(lua, ctx, class, None)?;
            let id = world::instance_id(&value)
                .ok_or_else(|| err("could not create builtin script Instance"))?;
            world::set_property(lua, ctx, &id, "Name", json!(path[index]))?;
            world::set_property(lua, ctx, &id, "Parent", json!({"$ref":parent}))?;
            created.insert(key.clone(), id.clone());
            if index + 1 == path.len() {
                state.borrow_mut().builtin_sources.insert(id, entry.clone());
            }
        }
    }
    Ok(())
}

fn prepare_templates(lua: &Lua, ctx: &WorldRef, state: &Rc<RefCell<Lifecycle>>) -> Result<()> {
    if state.borrow().templates_ready
        || state.borrow().actor_id.is_some()
        || !state.borrow().initial_ready
    {
        return Ok(());
    }
    let (player, character) = local_player_and_character(ctx);
    let Some(player) = player else {
        return Ok(());
    };
    let existing_player_scripts = world::children(ctx, &player).into_iter().find(|id| {
        ctx.borrow()
            .nodes
            .get(id)
            .is_some_and(|n| n.class_name == "PlayerScripts")
    });
    let player_scripts = match existing_player_scripts {
        Some(id) => Some(id),
        None => {
            let container = world::create_internal_node(
                lua,
                ctx,
                "PlayerScripts",
                Some(&player),
                Map::new(),
            )?;
            Some(world::instance_id(&container).ok_or_else(|| {
                err("engine PlayerScripts container was not created as an Instance")
            })?)
        }
    };
    let player_gui = world::children(ctx, &player).into_iter().find(|id| {
        ctx.borrow()
            .nodes
            .get(id)
            .is_some_and(|n| n.class_name == "PlayerGui")
    });
    let backpack = world::children(ctx, &player).into_iter().find(|id| {
        ctx.borrow()
            .nodes
            .get(id)
            .is_some_and(|n| n.class_name == "Backpack")
    });
    let (starter_player, starter_gui, starter_pack) = {
        let world = ctx.borrow();
        (
            world.services.get("StarterPlayer").cloned(),
            world.services.get("StarterGui").cloned(),
            world.services.get("StarterPack").cloned(),
        )
    };
    let starter_scripts = starter_player
        .as_ref()
        .and_then(|id| find_named_child(ctx, id, "StarterPlayerScripts"));
    let starter_character = starter_player
        .as_ref()
        .and_then(|id| find_named_child(ctx, id, "StarterCharacterScripts"));
    mirror_children(lua, ctx, state, starter_scripts, player_scripts.clone())?;
    mirror_children(lua, ctx, state, starter_character, character)?;
    mirror_children(lua, ctx, state, starter_gui, player_gui)?;
    mirror_children(lua, ctx, state, starter_pack, backpack)?;
    if let Some(target) = player_scripts {
        install_builtins(lua, ctx, state, &target)?;
    }
    state.borrow_mut().templates_ready = true;
    Ok(())
}

/// Reconcile eligible local scripts after replication or a local instance change.
pub fn reconcile(lua: &Lua, ctx: &WorldRef) -> Result<Json> {
    let state = lua
        .app_data_ref::<Rc<RefCell<Lifecycle>>>()
        .ok_or_else(|| err("script lifecycle is not installed"))?
        .clone();
    if let Some(ready) = ctx
        .borrow()
        .snapshot
        .get("initial_replication_complete")
        .and_then(Json::as_bool)
    {
        state.borrow_mut().initial_ready = ready;
    }
    if state.borrow().initial_ready {
        prepare_templates(lua, ctx, &state)?;
    }
    let admitted_script_ids: HashSet<String> = ctx
        .borrow()
        .nodes
        .values()
        .filter(|node| {
            !node.destroyed
                && matches!(
                    node.class_name.as_str(),
                    "Script" | "LocalScript" | "ModuleScript"
                )
        })
        .map(|node| node.id.clone())
        .collect();
    state
        .borrow_mut()
        .compiled_script_closures
        .retain(|id, _| admitted_script_ids.contains(id));
    let (player, character) = local_player_and_character(ctx);
    let actor = state.borrow().actor_id.clone();
    let script_ids: Vec<String> = ctx
        .borrow()
        .nodes
        .matching_class(|class| matches!(class, "Script" | "LocalScript"))
        .map(|node| node.id.clone())
        .collect();
    let mut starts = 0usize;
    for id in script_ids {
        let (should_run, reason) = {
            let (eligible, reason) = eligible(
                ctx,
                actor.as_deref(),
                &id,
                player.as_deref(),
                character.as_deref(),
            );
            (
                eligible && state.borrow().initial_ready,
                if state.borrow().initial_ready {
                    reason
                } else {
                    "initial_replication_incomplete"
                },
            )
        };
        let disabled = {
            let world = ctx.borrow();
            world.nodes.get(&id).is_none_or(|n| n.destroyed)
                || world.nodes.get(&id).is_some_and(|n| {
                    property(n, "Disabled").and_then(|v| v.as_bool()) == Some(true)
                        || property(n, "Enabled").and_then(|v| v.as_bool()) == Some(false)
                })
        };
        let active = state.borrow().scripts.get(&id).is_some_and(|e| e.active);
        let self_destroy_continuation = state.borrow().self_destroyed.contains(&id);
        if active && (!should_run || disabled) && !self_destroy_continuation {
            if let Some(thread) = state
                .borrow()
                .scripts
                .get(&id)
                .and_then(|e| e.thread.clone())
            {
                cancel_thread(lua, &thread)?;
            }
            if let Some(entry) = state.borrow_mut().scripts.get_mut(&id) {
                entry.active = false;
                entry.thread = None;
                entry.state = "stopped".into();
                entry.error = Some(if disabled {
                    "disabled".into()
                } else {
                    reason.into()
                });
            }
        }
        if should_run
            && !disabled
            && !state.borrow().scripts.get(&id).is_some_and(|e| e.active)
            && starts < 8
        {
            let instance = world::instance(lua, ctx, &id)?;
            match compile_for_instance(lua, ctx, &id, instance, &state) {
                Ok((function, environment)) => {
                    let thread = scheduler::enqueue_owned(
                        lua,
                        function,
                        MultiValue::new(),
                        format!("script:{id}"),
                    )?;
                    let key = lua.create_registry_value(environment)?;
                    state.borrow_mut().scripts.insert(
                        id.clone(),
                        ScriptEntry {
                            id: id.clone(),
                            thread: Some(thread),
                            environment: key,
                            state: "running".into(),
                            error: None,
                            active: true,
                        },
                    );
                    starts += 1;
                }
                Err(error) => {
                    let text = error.to_string();
                    let unavailable = text.contains("code_unavailable")
                        || text.contains("code unavailable")
                        || text.contains("verification_pending");
                    let classification = if unavailable { "blocked" } else { "failed" };
                    state.borrow_mut().scripts.insert(
                        id.clone(),
                        ScriptEntry {
                            id: id.clone(),
                            thread: None,
                            environment: lua.create_registry_value(lua.create_table()?)?,
                            state: classification.into(),
                            error: Some(text),
                            active: !unavailable,
                        },
                    );
                    if !unavailable {
                        starts += 1;
                    }
                }
            }
        }
    }
    {
        let mut state = state.borrow_mut();
        for entry in state.scripts.values_mut() {
            if entry.active {
                match entry.thread.as_ref().map(Thread::status) {
                    Some(ThreadStatus::Finished) => {
                        entry.state = "completed".into();
                    }
                    Some(ThreadStatus::Error) => {
                        entry.state = "failed".into();
                        entry.error = Some("script task failed".into());
                    }
                    _ => {}
                }
            }
        }
    }
    let status = {
        let state = state.borrow();
        status_json(&state)
    };
    Ok(status)
}

/// Validate and replace this VM's admitted source catalog.
pub fn update_sources(lua: &Lua, delta: &Json) -> Result<()> {
    // Session deltas carry source metadata as a top-level metadata field. An
    // ordinary actions/sequence delta without that field is not a catalog.
    if delta.get("script_sources").is_none()
        && (delta.get("actions").is_some() || delta.get("sequence").is_some())
    {
        return Ok(());
    }
    let value = delta.get("script_sources").unwrap_or(delta);
    let catalog = validate_catalog(value)?;
    let state = lua
        .app_data_ref::<Rc<RefCell<Lifecycle>>>()
        .ok_or_else(|| err("script lifecycle is not installed"))?
        .clone();
    state.borrow_mut().catalog = catalog;
    Ok(())
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "script_closure_tests.rs"]
mod script_closure_tests;

#[cfg(test)]
#[path = "lifecycle_shared_tests.rs"]
mod lifecycle_shared_tests;

#[cfg(test)]
#[path = "lifecycle_legacy_tests.rs"]
mod lifecycle_legacy_tests;
