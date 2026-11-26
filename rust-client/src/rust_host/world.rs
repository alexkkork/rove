//! Rust-owned world and Luau Instance handles.
//!
//! Server supplied state stays in the canonical projected world. Each `Node`
//! retains at most a projected per-record handle; only client-local writes are
//! copied into `local_overrides`.

use crate::engine_world_projection::ProjectedNode;
use crate::engine_world_view::SharedProjectedWorldView;
use mlua::{
    AnyUserData, Lua, MetaMethod, MultiValue, ObjectLike, Thread, UserData, UserDataFields,
    UserDataMethods, Value,
};
use rove_client_core::world::ReadOnlyField;
use serde_json::{json, Map as JsonMap, Value as Json};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::{Rc, Weak},
};

#[path = "world_pose_batch.rs"]
mod world_pose_batch;
#[path = "world_queries.rs"]
mod world_queries;
pub(crate) use world_pose_batch::{
    apply_pose_batch, apply_pose_command, pose_events, PoseWrite, EXPLICIT_PIVOT,
};
#[path = "world_model_scale.rs"]
mod world_model_scale;
pub(crate) use world_model_scale::apply_scale_batch;
#[path = "world_nodes.rs"]
mod world_nodes;
pub use world_nodes::NodeMap;
#[path = "world_data.rs"]
mod world_data;
pub use world_data::NodeData;
#[path = "world_globals.rs"]
mod world_globals;

pub type WorldRef = Rc<RefCell<World>>;
type InstanceHandleWaiters = Rc<RefCell<InstanceHandleWaitState>>;
#[derive(Default)]
struct InstanceHandleWaitState {
    next_id: u64,
    pending: Vec<InstanceHandleWaiter>,
}
struct InstanceHandleWaiter {
    token: u64,
    id: String,
    child_name: Option<String>,
    world: Weak<RefCell<World>>,
    thread: Thread,
    started: Instant,
    timeout_seconds: Option<f64>,
    timer: Option<Thread>,
}
pub type Method = fn(&Lua, &WorldRef, &str, MultiValue) -> mlua::Result<MultiValue>;
type MethodCallback = Rc<dyn Fn(&Lua, &WorldRef, &str, MultiValue) -> mlua::Result<MultiValue>>;
pub type Getter = fn(&Lua, &WorldRef, &str) -> mlua::Result<Option<Value>>;
pub type KeyedGetter = fn(&Lua, &WorldRef, &str, &str) -> mlua::Result<Option<Value>>;
pub type Setter = fn(&Lua, &WorldRef, &str, Value) -> mlua::Result<bool>;
pub type KeyedSetter = fn(&Lua, &WorldRef, &str, &str, Value) -> mlua::Result<bool>;

#[derive(Clone)]
pub struct Node {
    pub id: String,
    pub class_name: String,
    pub name: String,
    pub parent: Option<String>,
    pub destroyed: bool,
    pub server_owned: bool,
    pub received_creation_complete: bool,
    pub attributes_known: bool,
    pub collection_tags_known: bool,
    pub(crate) projected: Option<ProjectedNode>,
    pub data: Option<Box<NodeData>>,
}

pub struct World {
    pub nodes: NodeMap,
    /// Live reception tolerates updates for nodes skipped on the wire.
    pub tolerate_unknown_sets: bool,
    /// Sets that arrived before their node; replayed once it exists.
    pending_sets: Vec<Json>,
    pub services: HashMap<String, String>,
    pub snapshot: Json,
    pub(crate) camera_state: super::camera::CameraState,
    pub(crate) scale_state: super::movement::ScaleState,
    pub(crate) material_history: super::material_history::MaterialHistory,
    pub(crate) assembly_history: super::assembly_history::AssemblyHistory,
    pub(crate) curve_state: super::curves::CurveState,
    pub sequence: u64,
    /// Advances only when joint connectivity inputs change. Pose receipts can
    /// advance `sequence` many times without invalidating the rigid graph.
    pub(crate) joint_topology_revision: u64,
    /// Changes only when Actor runnable-source admission inputs change.
    pub(crate) actor_runnable_revision: u64,
    /// Root topology revision last merged into this Actor fork.
    actor_last_root_topology_revision: Option<u64>,
    pub(crate) script_catalog_generation: Arc<()>,
    lightweight_assets: bool,
    pub effects: JsonMap<String, Json>,
    pub focused_textbox: Option<String>,
    pub focused_text: String,
    pub focused_text_revision: u64,
    pub focused_editor_revision: u64,
    pub focused_cursor: i32,
    pub focused_anchor: i32,
    pub(crate) methods: HashMap<(String, String), MethodCallback>,
    pub(crate) getters: HashMap<String, Vec<Getter>>,
    pub(crate) keyed_getters: HashMap<String, Vec<KeyedGetter>>,
    pub(crate) setters: HashMap<String, Vec<Setter>>,
    pub(crate) keyed_setters: HashMap<String, Vec<KeyedSetter>>,
    local_namespace: String,
    next_local_id: u64,
    node_order: Vec<Arc<str>>,
    local_revision: u64,
}

/// Return the Roblox-style hierarchy path for an Instance. Descendants omit
/// the DataModel root (for example, `Workspace.Part`); asking for the
/// DataModel itself still returns its own name. Unparented instances use only
/// their own name, matching Instance:GetFullName().
pub(super) fn instance_full_name(world: &World, instance_id: &str) -> mlua::Result<String> {
    let mut names = Vec::new();
    let mut current = Some(instance_id);
    let mut seen = 0usize;

    while let Some(id) = current {
        let Some(node) = world.nodes.get(id) else {
            break;
        };
        // game is the container, not part of a descendant's full name.
        if node.class_name == "DataModel" && id != instance_id {
            break;
        }
        names.push(node.name.clone());
        if node.class_name == "DataModel" {
            break;
        }
        current = node.parent.as_deref();
        seen += 1;
        if seen > 400_000 {
            return Err(mlua::Error::RuntimeError(
                "parent cycle detected".into(),
            ));
        }
    }

    names.reverse();
    Ok(names.join("."))
}

impl World {
    pub(crate) fn header_order(&self) -> impl Iterator<Item = &str> {
        self.node_order.iter().map(AsRef::as_ref)
    }
    fn record_order(&mut self, id: &str) {
        self.node_order
            .push(self.nodes.shared_id(id).unwrap_or_else(|| Arc::from(id)));
    }

    /// Fork the current instance graph for a separate Luau state. Projected
    /// nodes keep only their individual shared record handles; VM registries,
    /// hook tables, effects, and the parent WorldView owner are not shared.
    pub fn fork_for_vm(&self) -> World {
        World {
            nodes: self.nodes.clone(),
            tolerate_unknown_sets: self.tolerate_unknown_sets,
            pending_sets: self.pending_sets.clone(),
            services: self.services.clone(),
            snapshot: self.snapshot.clone(),
            camera_state: self.camera_state.clone(),
            scale_state: self.scale_state.clone(),
            material_history: self.material_history.clone(),
            assembly_history: self.assembly_history.clone(),
            curve_state: self.curve_state.clone(),
            sequence: self.sequence,
            joint_topology_revision: self.joint_topology_revision,
            actor_runnable_revision: self.actor_runnable_revision,
            actor_last_root_topology_revision: self.actor_last_root_topology_revision,
            script_catalog_generation: self.script_catalog_generation.clone(),
            lightweight_assets: self.lightweight_assets,
            effects: JsonMap::new(),
            focused_textbox: self.focused_textbox.clone(),
            focused_text: self.focused_text.clone(),
            focused_text_revision: self.focused_text_revision,
            focused_editor_revision: self.focused_editor_revision,
            focused_cursor: self.focused_cursor,
            focused_anchor: self.focused_anchor,
            methods: HashMap::new(),
            getters: HashMap::new(),
            keyed_getters: HashMap::new(),
            setters: HashMap::new(),
            keyed_setters: HashMap::new(),
            local_namespace: self.local_namespace.clone(),
            next_local_id: self.next_local_id,
            node_order: self.node_order.clone(),
            local_revision: self.local_revision,
        }
    }

    pub fn fork_for_actor(&self, actor_id: &str) -> World {
        let mut fork = self.fork_for_vm();
        fork.actor_last_root_topology_revision = Some(self.joint_topology_revision);
        let mut encoded = String::with_capacity(actor_id.len() * 2);
        for byte in actor_id.as_bytes() {
            use std::fmt::Write as _;
            let _ = write!(&mut encoded, "{byte:02x}");
        }
        fork.local_namespace = format!("local:actor:{encoded}");
        fork
    }

    /// Refresh Actor state from root without flattening projected records or
    /// discarding Actor-local instances, overlays, and VM-owned registries.
    pub fn sync_actor_fork(&mut self, root: &World) {
        let root_topology_changed = self.actor_last_root_topology_revision
            != Some(root.joint_topology_revision);
        let mut topology_merge_changed = root_topology_changed;
        let material_root = (!self.material_history.shared_with(&root.material_history))
            .then(|| super::material_history::capture(root, None));
        let actor_prefix = format!("{}:", self.local_namespace);
        let actor_local_ids = self
            .nodes
            .ids()
            .filter(|id| id.starts_with(&actor_prefix) && !root.nodes.contains_key(*id))
            .map(str::to_owned)
            .collect::<Vec<_>>();

        // Warm the root's bounded header cache before forking it. Each Actor
        // then revalidates only its own overlay edits and local instances.
        root.nodes.orphan_ids();
        let mut merged = root.nodes.clone();
        for actor_node in self.nodes.materialized_values() {
            let id = &actor_node.id;
            if !root.nodes.contains_key(id) || actor_node.local_overrides.is_empty() {
                continue;
            }
            let Some(node) = merged.get_mut(id) else {
                continue;
            };
            // If another VM has committed a newer local write to this node,
            // keep the root version. Equal revisions are the same already-
            // merged state; greater Actor revisions are applied by its journal.
            if actor_node.local_revision < node.local_revision {
                topology_merge_changed |= actor_node
                    .local_overrides
                    .keys()
                    .any(|key| joint_topology_property(key));
                continue;
            }
            for (key, value) in &actor_node.local_overrides {
                // Physics observations are root-owned. In particular, a newer
                // received state can remove the old feedback override entirely.
                if matches!(
                    key.as_str(),
                    "__HumanoidStateFeedback" | "__HumanoidRunningFeedback"
                ) {
                    continue;
                }
                if node.class_name == "Humanoid"
                    && matches!(
                        key.as_str(),
                        "MoveDirection" | "__HumanoidMoveInput" | "__HumanoidMoveSequence"
                    )
                {
                    continue;
                }
                node.local_overrides.insert(key.clone(), value.clone());
            }
            if actor_node.local_overrides.contains_key("Name") {
                node.name = actor_node.name.clone();
            }
            if actor_node.local_overrides.contains_key("Parent") {
                node.parent = actor_node.parent.clone();
            }
            node.local_revision = actor_node.local_revision;
        }
        for id in &actor_local_ids {
            if let Some(node) = self.nodes.get(id) {
                merged.insert(id.clone(), node.clone());
            }
        }
        // Only orphaned headers need mutation. Reading every full node here
        // would expand the entire shared world on each Actor synchronization.
        let orphaned = merged.orphan_ids();
        // Clearing an orphan's parent can change endpoint ancestry or its
        // WorldRoot membership even when the root receipt cursor is unchanged.
        topology_merge_changed |= orphaned
            .iter()
            .any(|id| self.nodes.parent(id).flatten().is_some());
        for id in orphaned {
            merged.get_mut(&id).unwrap().parent = None;
        }

        self.nodes = merged;
        let mut order = root.node_order.clone();
        if !actor_local_ids.is_empty() {
            let mut ordered = order
                .iter()
                .cloned()
                .collect::<std::collections::HashSet<_>>();
            for id in actor_local_ids {
                let id = self
                    .nodes
                    .shared_id(&id)
                    .unwrap_or_else(|| Arc::from(id.as_str()));
                if ordered.insert(id.clone()) {
                    order.push(id);
                }
            }
        }
        self.node_order = order;
        self.services = root.services.clone();
        if let (Some(current), Some(next)) =
            (self.snapshot.as_object_mut(), root.snapshot.as_object())
        {
            current.retain(|key, _| next.contains_key(key));
            for (key, value) in next {
                if current.get(key) != Some(value) {
                    current.insert(key.clone(), value.clone());
                }
            }
        } else {
            self.snapshot = root.snapshot.clone();
        }
        self.script_catalog_generation = root.script_catalog_generation.clone();
        self.camera_state
            .sync_from_root(&root.camera_state, &self.local_namespace);
        self.scale_state
            .sync_from_root(&root.scale_state, &self.local_namespace);
        self.curve_state
            .sync_from_root(&root.curve_state, &self.local_namespace);
        self.sequence = root.sequence;
        if topology_merge_changed {
            self.joint_topology_revision = self.joint_topology_revision.wrapping_add(1);
        }
        self.actor_last_root_topology_revision = Some(root.joint_topology_revision);
        self.material_history = root.material_history.clone();
        self.assembly_history = root.assembly_history.clone();
        if let Some(before) = material_root {
            let after = super::material_history::capture(self, None);
            self.material_history
                .record(before, after, false, self.sequence);
        }
        self.local_revision = self.local_revision.max(root.local_revision);
        self.next_local_id = self.next_local_id.max(root.next_local_id);
    }

    pub fn from_snapshot(
        snapshot: &Json,
        shared: Option<&SharedProjectedWorldView>,
    ) -> Result<World, String> {
        let root = snapshot
            .as_object()
            .ok_or("world snapshot must be an object")?;
        if root.get("version").and_then(Json::as_u64) != Some(1) {
            return Err("unsupported or missing world snapshot version".into());
        }
        let sequence = root.get("sequence").and_then(Json::as_u64).unwrap_or(0);
        let tolerate_unknown_sets =
            std::env::var_os("ROVE_TOLERATE_UNKNOWN_SETS").is_some_and(|v| v != "0");
        let display = super::camera::display_update(root)?;
        let records = root.get("nodes").and_then(Json::as_array);
        if records.is_none() && shared.is_none() {
            return Err("world snapshot nodes must be an array without a projected view".into());
        }
        let node_count = records.filter(|nodes| !nodes.is_empty()).map_or_else(
            || shared.map_or(0, SharedProjectedWorldView::node_count),
            Vec::len,
        );
        if node_count > 400_000 {
            return Err("world snapshot exceeds node limit".into());
        }
        let mut world = World {
            nodes: NodeMap::with_capacity(node_count),
            tolerate_unknown_sets,
            pending_sets: Vec::new(),
            services: HashMap::new(),
            snapshot: Json::Null,
            camera_state: super::camera::CameraState::default(),
            scale_state: super::movement::ScaleState::default(),
            material_history: super::material_history::MaterialHistory::default(),
            assembly_history: super::assembly_history::AssemblyHistory::default(),
            curve_state: super::curves::CurveState::default(),
            sequence,
            joint_topology_revision: 0,
            actor_runnable_revision: 0,
            actor_last_root_topology_revision: None,
            script_catalog_generation: Arc::new(()),
            lightweight_assets: root
                .get("lightweight_assets")
                .and_then(Json::as_bool)
                .unwrap_or(false),
            effects: JsonMap::new(),
            focused_textbox: None,
            focused_text: String::new(),
            focused_text_revision: 0,
            focused_editor_revision: 0,
            focused_cursor: 0,
            focused_anchor: 0,
            methods: HashMap::new(),
            getters: HashMap::new(),
            keyed_getters: HashMap::new(),
            setters: HashMap::new(),
            keyed_setters: HashMap::new(),
            local_namespace: "local".into(),
            next_local_id: 1,
            node_order: Vec::with_capacity(node_count),
            local_revision: 0,
        };
        for record in records.into_iter().flatten() {
            let obj = record.as_object().ok_or("world node must be an object")?;
            let id = required_string(obj.get("id"), "node id")?;
            if world.nodes.contains_key(&id) {
                return Err(format!("duplicate world node id {id:?}"));
            }
            let projected = shared.and_then(|v| v.lookup_node(&id));
            let class_name = obj
                .get("class_name")
                .and_then(Json::as_str)
                .or_else(|| projected.as_ref().map(ProjectedNode::class_name))
                .ok_or("world node class_name must be a string")?
                .to_owned();
            let name = match obj.get("name").and_then(Json::as_str) {
                Some(name) => name.to_owned(),
                None => projected
                    .as_ref()
                    .and_then(|p| p.name().ok())
                    .unwrap_or_else(|| class_name.clone()),
            };
            let parent = match obj.get("parent") {
                None | Some(Json::Null) => None,
                Some(Json::String(s)) => Some(s.clone()),
                Some(Json::Object(m)) => {
                    m.get("$ref").and_then(Json::as_str).map(ToOwned::to_owned)
                }
                _ => return Err("world node parent must be a string or null".into()),
            };
            let destroyed = obj
                .get("destroyed")
                .and_then(Json::as_bool)
                .unwrap_or(false);
            let server_owned = obj
                .get("server_owned")
                .and_then(Json::as_bool)
                .unwrap_or(true);
            let attributes_known = obj
                .get("attributes_known")
                .and_then(Json::as_bool)
                .unwrap_or(obj.contains_key("attributes"));
            let attributes = obj
                .get("attributes")
                .and_then(Json::as_object)
                .cloned()
                .unwrap_or_default();
            let revisions = obj
                .get("property_revisions")
                .and_then(Json::as_object)
                .cloned()
                .unwrap_or_default();
            let mut properties = if projected.is_some() {
                JsonMap::new()
            } else {
                obj.get("properties")
                    .and_then(Json::as_object)
                    .cloned()
                    .unwrap_or_default()
            };
            if projected.is_none() {
                received_complete_create_defaults(obj, &class_name, &mut properties);
            }
            let collection_tags = obj
                .get("collection_tags")
                .and_then(Json::as_array)
                .map(|v| {
                    v.iter()
                        .filter_map(Json::as_str)
                        .map(ToOwned::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            let collection_tags_known = obj
                .get("collection_tags_known")
                .and_then(Json::as_bool)
                .unwrap_or(obj.contains_key("collection_tags"));
            let node = Node {
                id: id.clone(),
                class_name: class_name.clone(),
                name,
                parent,
                destroyed,
                server_owned,
                received_creation_complete: obj.get("received_creation_complete") == Some(&json!(true)),
                attributes_known,
                collection_tags_known,
                projected,
                data: crate::rust_host::world::NodeData {
                    properties,
                    local_overrides: JsonMap::new(),
                    attributes,
                    revisions,
                    local_revision: 0,
                    collection_tags,
                }
                .pack(),
            };
            if let Some(projected) = &node.projected {
                for wire_key in ["Color3uint8", "CollisionGroupReplicate"] {
                    if let Some(value) = projected.property(wire_key)? {
                        let _ =
                            super::world_aliases::from_wire(&node.class_name, wire_key, &value)?;
                    }
                }
            }
            world.nodes.insert(id, node);
            world.record_order(obj.get("id").and_then(Json::as_str).unwrap());
            if class_name.ends_with("Service")
                || matches!(
                    class_name.as_str(),
                    "Workspace" | "Players" | "RunService" | "UserSettings"
                )
            {
                world.services.entry(class_name).or_insert_with(|| {
                    required_string(obj.get("id"), "node id").unwrap_or_default()
                });
            }
        }
        if world.nodes.is_empty() {
            if let Some(shared) = shared {
                shared.for_each_node(|projected| {
                    let id = projected.id().to_owned();
                    let class_name = projected.class_name().to_owned();
                    let name = projected.name().unwrap_or_else(|_| class_name.clone());
                    let parent = match projected.parent() {
                        ReadOnlyField::Value(value) => Some(value),
                        ReadOnlyField::Missing | ReadOnlyField::Null => None,
                    };
                    let record = projected.raw();
                    let destroyed = record
                        .record_field("destroyed")
                        .and_then(|value| value.as_bool())
                        .unwrap_or(false);
                    let server_owned = record
                        .record_field("server_owned")
                        .and_then(|value| value.as_bool())
                        .unwrap_or(true);
                    let attributes = match projected.section("attributes") {
                        Ok(ReadOnlyField::Value(Json::Object(map))) => map,
                        _ => JsonMap::new(),
                    };
                    let attributes_known = record
                        .record_field("attributes_known")
                        .and_then(|value| value.as_bool())
                        .unwrap_or(false);
                    let collection_tags = match projected.section("collection_tags") {
                        Ok(ReadOnlyField::Value(Json::Array(values))) => values
                            .iter()
                            .filter_map(Json::as_str)
                            .map(ToOwned::to_owned)
                            .collect(),
                        _ => Vec::new(),
                    };
                    let collection_tags_known = record
                        .record_field("collection_tags_known")
                        .and_then(|value| value.as_bool())
                        .unwrap_or(false);
                    // Received revision values already live in the projected
                    // record. The local map only tracks later override writes.
                    let revisions = JsonMap::new();
                    world.record_order(&id);
                    if class_name.ends_with("Service")
                        || matches!(
                            class_name.as_str(),
                            "Workspace" | "Players" | "RunService" | "UserSettings"
                        )
                    {
                        world
                            .services
                            .entry(class_name.clone())
                            .or_insert_with(|| id.clone());
                    }
                    world.nodes.insert(
                        id.clone(),
                        Node {
                            id,
                            class_name,
                            name,
                            parent,
                            destroyed,
                            server_owned,
                            received_creation_complete: record.record_field("received_creation_complete") == Some(json!(true)),
                            attributes_known,
                            collection_tags_known,
                            projected: Some(projected),
                            data: crate::rust_host::world::NodeData {
                                properties: JsonMap::new(),
                                local_overrides: JsonMap::new(),
                                attributes,
                                revisions,
                                local_revision: 0,
                                collection_tags,
                            }
                            .pack(),
                        },
                    );
                });
            }
        }
        if let Some(services) = root.get("services").and_then(Json::as_object) {
            for (name, value) in services {
                let id = value
                    .as_str()
                    .or_else(|| value.get("$ref").and_then(Json::as_str))
                    .ok_or("service values must be node IDs or $ref records")?;
                world.services.insert(name.clone(), id.to_owned());
            }
        }
        let data_models = world
            .nodes
            .of_class("DataModel")
            .filter(|node| node.class_name == "DataModel")
            .count();
        if data_models > 1 {
            return Err("snapshot has multiple DataModel nodes".into());
        }
        if data_models == 0 {
            // A projected snapshot can arrive before its DataModel record.
            // This local root supplies the host's service lookup surface.
            let id = "__world_snapshot_datamodel__".to_owned();
            if world.nodes.contains_key(&id) {
                return Err("reserved local DataModel identity is occupied".into());
            }
            world.record_order(&id);
            world.nodes.insert(
                id.clone(),
                Node {
                    id,
                    class_name: "DataModel".into(),
                    name: "Game".into(),
                    parent: None,
                    destroyed: false,
                    server_owned: false,
                    received_creation_complete: false,
                    attributes_known: true,
                    collection_tags_known: true,
                    projected: None,
                    data: crate::rust_host::world::NodeData {
                        properties: JsonMap::new(),
                        local_overrides: JsonMap::new(),
                        attributes: JsonMap::new(),
                        revisions: JsonMap::new(),
                        local_revision: 0,
                        collection_tags: Vec::new(),
                    }
                    .pack(),
                },
            );
        }
        let game_id = world
            .nodes
            .of_class("DataModel")
            .find(|node| node.class_name == "DataModel")
            .map(|node| node.id.clone())
            .ok_or("snapshot DataModel could not be initialized")?;
        for id in world.services.values() {
            if id == &game_id || !world.nodes.contains_key(id) {
                return Err("service references an unknown node".into());
            }
            if let Some(node) = world.nodes.get_mut(id) {
                if node.parent.is_none() && !node.destroyed {
                    node.parent = Some(game_id.clone());
                }
            }
        }
        world.snapshot = Json::Object(
            root.iter()
                .filter(|(key, _)| key.as_str() != "nodes")
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        );
        if let Some(display) = display {
            world.snapshot["display"] = display;
        }
        Ok(world)
    }

    pub fn apply_delta(
        &mut self,
        delta: &Json,
        shared: Option<&SharedProjectedWorldView>,
    ) -> Result<(), String> {
        // Keep one before-image per modified node. Cloning the complete graph
        // for every receipt doubled its storage even for a one-property update.
        // Callbacks run only after this transaction has successfully committed.
        let assembly_causes = super::assembly_history::Causes::received(self, delta);
        let assembly_before = assembly_causes
            .as_ref()
            .map(|causes| super::assembly_history::capture(self, causes));
        let material_causes = super::material_history::Causes::received(self, delta);
        let material_before = material_causes
            .as_ref()
            .map(|causes| super::material_history::capture(self, Some(causes)));
        let assembly_history_before = self.assembly_history.clone();
        let mut before_nodes = HashMap::new();
        let before_services = self.services.clone();
        let before_sequence = self.sequence;
        let before_order_len = self.node_order.len();
        let before_metadata = delta
            .as_object()
            .into_iter()
            .flat_map(|root| root.keys())
            .filter(|key| !matches!(key.as_str(), "actions" | "version" | "delta"))
            .map(|key| (key.clone(), self.snapshot.get(key).cloned()))
            .collect::<Vec<_>>();
        if let Err(error) = self.apply_delta_in_place(delta, shared, &mut before_nodes) {
            for (id, node) in before_nodes {
                match node {
                    Some(node) => {
                        self.nodes.insert(id, node);
                    }
                    None => {
                        self.nodes.remove(&id);
                    }
                }
            }
            self.assembly_history = assembly_history_before;
            self.services = before_services;
            self.sequence = before_sequence;
            self.node_order.truncate(before_order_len);
            if let Some(snapshot) = self.snapshot.as_object_mut() {
                for (key, value) in before_metadata {
                    match value {
                        Some(value) => {
                            snapshot.insert(key, value);
                        }
                        None => {
                            snapshot.remove(&key);
                        }
                    }
                }
            }
            return Err(error);
        }
        if let Some(before) = assembly_before {
            let after = super::assembly_history::capture(self, assembly_causes.as_ref().unwrap());
            self.assembly_history.record(before, after, self.sequence);
        }
        if let Some(before) = material_before {
            let after = super::material_history::capture(self, material_causes.as_ref());
            self.material_history
                .record(before, after, true, self.sequence);
        }
        let topology_changed = delta["actions"].as_array().is_some_and(|actions| {
            actions.iter().any(|action| {
                action["sequence"].as_u64().unwrap_or(0) > before_sequence
                    && (matches!(action["kind"].as_str(), Some("create" | "remove" | "destroy"))
                        || (matches!(action["kind"].as_str(), Some("set" | "set_property"))
                            && action_topology_fields(action)))
            })
        });
        if topology_changed {
            self.joint_topology_revision = self.joint_topology_revision.wrapping_add(1);
        }
        let runnable_changed = delta.get("script_sources").is_some()
            || delta.get("initial_replication_complete").is_some()
            || delta["actions"].as_array().is_some_and(|actions| {
                actions.iter().any(|action| {
                    action["sequence"].as_u64().unwrap_or(0) > before_sequence
                        && action_affects_actor_runnable(action)
                })
            });
        if runnable_changed {
            self.actor_runnable_revision = self.actor_runnable_revision.wrapping_add(1);
        }
        if delta.get("script_sources").is_some() {
            self.script_catalog_generation = Arc::new(());
        }
        for id in before_nodes.keys() {
            self.nodes.compact_destroyed(id);
        }
        Ok(())
    }

    fn apply_delta_in_place(
        &mut self,
        delta: &Json,
        shared: Option<&SharedProjectedWorldView>,
        before_nodes: &mut HashMap<String, Option<Node>>,
    ) -> Result<(), String> {
        let root = delta.as_object().ok_or("world delta must be an object")?;
        let display = super::camera::display_update(root)?;
        // Replay sets whose node has since arrived.
        if self.tolerate_unknown_sets && !self.pending_sets.is_empty() {
            let pending = std::mem::take(&mut self.pending_sets);
            let mut still_missing = Vec::new();
            for action in pending {
                let Some(id) = action.get("id").and_then(Json::as_str) else { continue };
                if self.nodes.contains_key(id) {
                    let fields = action.get("fields").and_then(Json::as_object).cloned()
                        .unwrap_or_default();
                    if let Some(node) = self.nodes.get_mut(id) {
                        for (key, value) in fields {
                            if key == "Parent" {
                                node.parent = ref_id(&value);
                            } else if key == "Name" {
                                if let Some(name) = value.as_str() {
                                    node.name = name.to_owned();
                                }
                            } else {
                                node.properties.insert(key, value);
                            }
                        }
                    }
                } else {
                    still_missing.push(action);
                }
            }
            self.pending_sets = still_missing;
        }
        if !super::session::json_fits(delta, 32 * 1024 * 1024) {
            return Err("world delta exceeds 8 MiB".into());
        }
        let target = root
            .get("sequence")
            .and_then(Json::as_u64)
            .ok_or("world delta sequence must be an unsigned integer")?;
        if root.get("version").and_then(Json::as_u64) != Some(1)
            || root.get("delta").and_then(Json::as_bool) != Some(true)
        {
            return Err("unsupported world delta envelope".into());
        }
        if target < self.sequence {
            return Err("world delta sequence moved backwards".into());
        }
        let actions = root
            .get("actions")
            .and_then(Json::as_array)
            .ok_or("world delta actions must be an array")?;
        if actions.len() > 10_000 {
            return Err("world delta action limit exceeded".into());
        }
        if let Some(player) = root.get("local_player") {
            if !player.is_null() && !player.is_string() {
                return Err("local_player must be a session-owned reference or null".into());
            }
            if let Some(id) = player.as_str() {
                let created = actions.iter().rev().find_map(|action| {
                    if action.get("kind").and_then(Json::as_str) != Some("create") {
                        return None;
                    }
                    let node = action.get("node")?;
                    (node.get("id").and_then(Json::as_str) == Some(id)).then_some(node)
                });
                let class = created
                    .and_then(|node| node.get("class_name").and_then(Json::as_str))
                    .or_else(|| {
                        self.nodes
                            .get(id)
                            .filter(|node| !node.destroyed)
                            .map(|node| node.class_name.as_str())
                    });
                if class != Some("Player") {
                    return Err("local_player must reference a live Player".into());
                }
            }
        }
        if let Some(value) = root.get("services") {
            let services = value
                .as_object()
                .ok_or("delta services must be an object")?;
            if services.len() > 128 {
                return Err("delta services exceed the service limit".into());
            }
            for (name, value) in services {
                if name.is_empty()
                    || name.len() > 256
                    || (!value.is_null() && !value.is_string())
                    || value.as_str().is_some_and(str::is_empty)
                {
                    return Err("delta service entries must contain instance ids or null".into());
                }
            }
        }
        let mut last_action_sequence = self.sequence;
        for action in actions {
            let a = action
                .as_object()
                .ok_or("world delta action must be an object")?;
            let kind = a
                .get("kind")
                .and_then(Json::as_str)
                .ok_or("world delta action kind is missing")?;
            let action_sequence = a
                .get("sequence")
                .and_then(Json::as_u64)
                .ok_or("world delta action sequence is missing")?;
            if action_sequence <= self.sequence {
                continue;
            }
            if action_sequence <= last_action_sequence || action_sequence > target {
                return Err(
                    "world delta action sequence is unordered or beyond its envelope".into(),
                );
            }
            last_action_sequence = action_sequence;
            match kind {
                "create" => {
                    let record = a
                        .get("node")
                        .and_then(Json::as_object)
                        .ok_or("create action node is missing")?;
                    let id = required_string(record.get("id"), "node id")?;
                    self.assembly_history.forget_removed(&id);
                    let class_name = required_string(record.get("class_name"), "node class_name")?;
                    let name = record
                        .get("name")
                        .and_then(Json::as_str)
                        .unwrap_or(&class_name)
                        .to_owned();
                    let parent = record
                        .get("parent")
                        .and_then(Json::as_str)
                        .map(ToOwned::to_owned);
                    let projected = shared.and_then(|v| v.lookup_node(&id));
                    let mut properties = if projected.is_some() {
                        JsonMap::new()
                    } else {
                        record
                            .get("properties")
                            .and_then(Json::as_object)
                            .cloned()
                            .unwrap_or_default()
                    };
                    if projected.is_none() {
                        received_complete_create_defaults(record, &class_name, &mut properties);
                    }
                    let revisions = record
                        .get("property_revisions")
                        .and_then(Json::as_object)
                        .cloned()
                        .unwrap_or_default();
                    let collection_tags = record
                        .get("collection_tags")
                        .and_then(Json::as_array)
                        .map(|v| {
                            v.iter()
                                .filter_map(Json::as_str)
                                .map(ToOwned::to_owned)
                                .collect()
                        })
                        .unwrap_or_default();
                    let collection_tags_known = record
                        .get("collection_tags_known")
                        .and_then(Json::as_bool)
                        .unwrap_or(false);
                    let node = Node {
                        id: id.clone(),
                        class_name,
                        name,
                        parent,
                        destroyed: false,
                        server_owned: true,
                        received_creation_complete: record.get("received_creation_complete") == Some(&json!(true)),
                        attributes_known: record
                            .get("attributes_known")
                            .and_then(Json::as_bool)
                            .unwrap_or(false),
                        collection_tags_known,
                        projected,
                        data: crate::rust_host::world::NodeData {
                            properties,
                            local_overrides: JsonMap::new(),
                            attributes: record
                                .get("attributes")
                                .and_then(Json::as_object)
                                .cloned()
                                .unwrap_or_default(),
                            revisions,
                            local_revision: 0,
                            collection_tags,
                        }
                        .pack(),
                    };
                    let is_new = !self.nodes.contains_key(&id);
                    remember_node(&self.nodes, before_nodes, &id);
                    self.nodes.insert(id, node);
                    if is_new {
                        self.record_order(record.get("id").and_then(Json::as_str).unwrap());
                    }
                }
                "set" | "set_property" => {
                    let id = required_string(a.get("id"), "set id")?;
                    let mut fields = JsonMap::new();
                    if let Some(set_fields) = a.get("fields").and_then(Json::as_object) {
                        fields.extend(set_fields.iter().map(|(k, v)| (k.clone(), v.clone())));
                    }
                    if let Some(key) = a
                        .get("property")
                        .or_else(|| a.get("key"))
                        .and_then(Json::as_str)
                    {
                        fields.insert(
                            key.into(),
                            a.get("value").cloned().ok_or("set value is missing")?,
                        );
                    }
                    remember_node(&self.nodes, before_nodes, &id);
                    let Some(node) = self.nodes.get_mut(&id) else {
                        if self.tolerate_unknown_sets {
                            if self.pending_sets.len() >= 20_000 {
                                self.pending_sets.remove(0);
                            }
                            self.pending_sets.push(Json::Object(a.clone()));
                            continue;
                        }
                        return Err("set references unknown node".to_string());
                    };
                    super::movement::clear_received_attachment_overrides(
                        node,
                        &fields,
                        action_sequence,
                    )?;
                    super::movement::remember_received_motor(node, &fields, action_sequence);
                    for (key, value) in fields {
                        let alias = super::world_aliases::canonical_key(&node.class_name, &key);
                        if alias.is_some() {
                            let _ =
                                super::world_aliases::from_wire(&node.class_name, &key, &value)?;
                        }
                        if key == "Parent" {
                            node.parent = ref_id(&value);
                        } else if key == "Name" {
                            if let Some(s) = value.as_str() {
                                node.name = s.into();
                            }
                        }
                        clear_received_property_override(node, &key);
                        if shared.and_then(|v| v.lookup_node(&id)).is_none() {
                            node.properties.insert(key.clone(), value);
                            node.revisions.insert(key, json!(action_sequence));
                        } else if let Some(projected) = shared.and_then(|v| v.lookup_node(&id)) {
                            match projected.revision(&key).ok().flatten() {
                                Some(revision) => {
                                    node.revisions.insert(key, revision);
                                }
                                None => {
                                    node.revisions.remove(&key);
                                }
                            }
                        }
                    }
                    if let Some(projected) = shared.and_then(|v| v.lookup_node(&id)) {
                        node.projected = Some(projected);
                    }
                    if let Some(snapshot) = a.get("attribute_snapshot") {
                        let attrs = snapshot
                            .get("attributes")
                            .and_then(Json::as_object)
                            .ok_or("attribute snapshot values must be an object")?;
                        if attrs.len() > 1024 {
                            return Err("attribute count limit exceeded".into());
                        }
                        node.attributes = attrs.clone();
                        node.attributes_known = snapshot
                            .get("attributes_known")
                            .and_then(Json::as_bool)
                            .ok_or("attribute snapshot known flag is missing")?;
                    }
                    if let Some(snapshot) = a.get("tag_snapshot") {
                        let tags = snapshot
                            .get("collection_tags")
                            .and_then(Json::as_array)
                            .ok_or("collection tag snapshot values must be an array")?;
                        node.collection_tags_known = snapshot
                            .get("collection_tags_known")
                            .and_then(Json::as_bool)
                            .unwrap_or(true);
                        node.collection_tags = validate_tags(tags, node.collection_tags_known)?;
                    }
                }
                "attribute" => {
                    let id = required_string(a.get("id"), "attribute id")?;
                    let update = a
                        .get("attribute_update")
                        .and_then(Json::as_object)
                        .ok_or("attribute update is missing")?;
                    let key = required_string(update.get("name"), "attribute name")?;
                    remember_node(&self.nodes, before_nodes, &id);
                    let node = self
                        .nodes
                        .get_mut(&id)
                        .ok_or("attribute update references unknown node")?;
                    let known = update.get("known").and_then(Json::as_bool).unwrap_or(false);
                    if known {
                        let value = update
                            .get("value")
                            .cloned()
                            .ok_or("known attribute update value is missing")?;
                        if !node.attributes.contains_key(&key)
                            && (!value.is_null() || !node.attributes_known)
                            && node.attributes.len() >= 1024
                        {
                            return Err("attribute count limit exceeded".into());
                        }
                        node.attributes.insert(key, value);
                    } else {
                        node.attributes.remove(&key);
                        node.attributes_known = false;
                    }
                    // A known single key does not establish that the entire
                    // attribute map was replicated. Only a full snapshot does.
                }
                "set_tags" => {
                    let id = required_string(a.get("id"), "tag update id")?;
                    let tags = a
                        .get("tags")
                        .and_then(Json::as_array)
                        .ok_or("tag update list is missing")?;
                    validate_tags(tags, true)?;
                    remember_node(&self.nodes, before_nodes, &id);
                    let Some(node) = self.nodes.get_mut(&id) else {
                        continue;
                    };
                    node.collection_tags = validate_tags(tags, true)?;
                    node.collection_tags_known = true;
                }
                "remove" | "destroy" => {
                    let roots = if let Some(id) = a.get("id").and_then(Json::as_str) {
                        vec![id.to_owned()]
                    } else if let Some(ids) = a.get("ids").and_then(Json::as_array) {
                        ids.iter()
                            .filter_map(Json::as_str)
                            .map(ToOwned::to_owned)
                            .collect()
                    } else {
                        return Err("destroy action ids are missing".into());
                    };
                    let mut pending = roots;
                    let mut selected = Vec::new();
                    let mut seen = std::collections::HashSet::new();
                    while let Some(id) = pending.pop() {
                        if !seen.insert(id.clone()) {
                            continue;
                        }
                        if !self.nodes.contains_key(&id) {
                            continue;
                        }
                        selected.push(id.clone());
                        pending.extend(self.nodes.children(&id, &self.node_order));
                        if selected.len() > 400_000 {
                            return Err("destroy subtree limit exceeded".into());
                        }
                    }
                    for id in selected {
                        remember_node(&self.nodes, before_nodes, &id);
                        if self
                            .nodes
                            .class_name(&id)
                            .is_some_and(super::joint_graph::rigid_class)
                        {
                            let shared = self
                                .nodes
                                .shared_id(&id)
                                .unwrap_or_else(|| Arc::from(id.as_str()));
                            self.assembly_history
                                .confirm_removed(shared, action_sequence);
                        }
                        if let Some(node) = self.nodes.get_mut(&id) {
                            node.destroyed = true;
                            node.parent = None;
                            node.local_overrides.insert("Parent".into(), Json::Null);
                        }
                    }
                }
                "event" => {
                    required_string(a.get("id"), "event id")?;
                    required_string(a.get("name"), "event name")?;
                    if !a.get("arguments").is_some_and(Json::is_array) {
                        return Err("event arguments must be an array".into());
                    }
                }
                other => return Err(format!("unsupported world delta action {other:?}")),
            }
        }
        self.sequence = target;
        self.snapshot["sequence"] = json!(target);
        for (key, value) in root {
            if matches!(key.as_str(), "actions" | "sequence" | "version" | "delta") {
                continue;
            }
            if key == "services" {
                if let Some(services) = value.as_object() {
                    for (name, value) in services {
                        if value.is_null() {
                            self.services.remove(name);
                            continue;
                        }
                        let id = value
                            .as_str()
                            .ok_or("delta service reference must be a node ID or null")?;
                        let game_id = self
                            .nodes
                            .of_class("DataModel")
                            .find(|node| node.class_name == "DataModel")
                            .map(|node| node.id.clone())
                            .ok_or("delta DataModel is unavailable")?;
                        remember_node(&self.nodes, before_nodes, id);
                        let service = self
                            .nodes
                            .get_mut(id)
                            .ok_or("delta service reference is invalid")?;
                        if service.destroyed
                            || service.class_name != name.as_str()
                            || id == game_id.as_str()
                        {
                            return Err("delta service reference is invalid".into());
                        }
                        if service.parent.is_none() {
                            service.parent = Some(game_id);
                        }
                        self.services.insert(name.clone(), id.to_owned());
                    }
                }
            }
            if let Some(snapshot) = self.snapshot.as_object_mut() {
                snapshot.insert(key.clone(), value.clone());
            }
        }
        let nodes = &self.nodes;
        self.services.retain(|name, id| {
            nodes
                .get(id)
                .is_some_and(|node| !node.destroyed && node.class_name == name.as_str())
        });
        if let Some(display) = display {
            self.snapshot["display"] = display;
        }
        Ok(())
    }
}

fn remember_node(nodes: &NodeMap, before: &mut HashMap<String, Option<Node>>, id: &str) {
    if !before.contains_key(id) {
        before.insert(id.to_owned(), nodes.get(id).cloned());
    }
}

fn required_string(value: Option<&Json>, what: &str) -> Result<String, String> {
    value
        .and_then(Json::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("{what} must be a string"))
}

fn local_instance_id(namespace: &str, counter: u64) -> String {
    format!("{namespace}:{counter}")
}

fn allocate_local_instance_id(world: &mut World) -> mlua::Result<String> {
    loop {
        let id = local_instance_id(&world.local_namespace, world.next_local_id);
        world.next_local_id = world
            .next_local_id
            .checked_add(1)
            .ok_or_else(|| mlua::Error::RuntimeError("local instance ID exhausted".into()))?;
        if !world.nodes.contains_key(&id) {
            return Ok(id);
        }
    }
}
fn ref_id(value: &Json) -> Option<String> {
    value.as_str().map(ToOwned::to_owned).or_else(|| {
        value
            .get("$ref")
            .and_then(Json::as_str)
            .map(ToOwned::to_owned)
    })
}

fn validate_tags(values: &[Json], known: bool) -> Result<Vec<String>, String> {
    if values.len() > 256 {
        return Err("instance tag limit exceeded".into());
    }
    let mut tags = values
        .iter()
        .map(|value| {
            let tag = value
                .as_str()
                .ok_or_else(|| "collection tag must be a string".to_owned())?;
            if tag.is_empty() || tag.len() > 65_536 || tag.contains('\0') {
                return Err("invalid collection tag".to_owned());
            }
            Ok(tag.to_owned())
        })
        .collect::<Result<Vec<_>, String>>()?;
    if !known && !tags.is_empty() {
        return Err("unknown tag state cannot contain values".into());
    }
    tags.sort();
    if tags.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err("duplicate collection tag".into());
    }
    Ok(tags)
}

pub(crate) fn node_property(node: &Node, key: &str) -> Option<Json> {
    if let Some(value) = node.local_overrides.get(key) {
        return Some(value.clone());
    }
    if let Some(value) = super::movement::received_joint_property(node, key) {
        return Some(value);
    }
    match key {
        "Name" => return Some(json!(node.name)),
        "ClassName" => return Some(json!(node.class_name)),
        "Parent" => {
            return Some(
                node.parent
                    .as_ref()
                    .map(|id| json!({"$ref":id}))
                    .unwrap_or(Json::Null),
            );
        }
        _ => {}
    }
    if let Some(projected) = &node.projected {
        return projected.property(key).ok().flatten();
    }
    node.properties.get(key).cloned()
}

pub(crate) fn attribute_value_eq(left: &Json, right: &Json) -> bool {
    match (left, right) {
        (Json::Number(a), Json::Number(b)) => a.as_f64() == b.as_f64(),
        (Json::Array(a), Json::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| attribute_value_eq(a, b))
        }
        (Json::Object(a), Json::Object(b)) => {
            a.len() == b.len()
                && a.iter().all(|(key, value)| {
                    b.get(key)
                        .is_some_and(|other| attribute_value_eq(value, other))
                })
        }
        _ => left == right,
    }
}

/// Apply a receipt and then publish its local event effects only after the
/// world borrow has been released. The state update itself is transactional.
pub fn apply_delta_with_lua(
    lua: &Lua,
    ctx: &WorldRef,
    delta: &Json,
    shared: Option<&SharedProjectedWorldView>,
) -> mlua::Result<()> {
    let actions = delta
        .get("actions")
        .and_then(Json::as_array)
        .ok_or_else(|| mlua::Error::RuntimeError("world delta actions must be an array".into()))?;
    let starting_sequence = ctx.borrow().sequence;
    let camera_before = super::camera::observe_delta(ctx, delta);
    let (previous_player, previously_loaded) = {
        let world = ctx.borrow();
        (
            world
                .snapshot
                .get("local_player")
                .cloned()
                .unwrap_or(Json::Null),
            world.snapshot["initial_replication_complete"]
                .as_bool()
                .unwrap_or(false),
        )
    };
    let mut state_events = super::humanoid_state::prepare(ctx, actions, Some(starting_sequence))?;
    let mut health_events = super::humanoid_health::prepare(ctx, actions, Some(starting_sequence));
    let mut property_events =
        super::property_events::prepare(lua, ctx, actions, Some(starting_sequence));
    let joint_registration =
        super::movement::before_received_joint_poses(lua, ctx, actions, starting_sequence)?;
    let mut player_events =
        super::player_lifecycle::prepare(lua, ctx, actions, Some(starting_sequence), shared);
    let mut collection_events =
        super::collection_events::prepare(ctx, actions, Some(starting_sequence));
    let mut attribute_events =
        super::attribute_events::prepare(ctx, actions, Some(starting_sequence));
    let mut hierarchy_events =
        super::hierarchy_events::prepare(lua, ctx, actions, Some(starting_sequence))?;
    let mut destroyed_nodes = Vec::new();
    ctx.borrow_mut()
        .apply_delta(delta, shared)
        .map_err(mlua::Error::RuntimeError)?;
    player_events.commit();
    super::geometry::mass_events::synchronize(lua, ctx, &property_events)?;
    super::curves::cleanup_received(ctx);
    super::camera::refresh_after_delta(lua, ctx, delta, camera_before)?;
    resolve_instance_handle_waiters(lua, ctx)?;
    let game = world_globals::refresh(
        lua,
        ctx,
        Some(starting_sequence),
        actions.iter().any(|action| {
            action["sequence"].as_u64().unwrap_or(0) > starting_sequence
                && matches!(
                    action["kind"].as_str(),
                    Some("create" | "remove" | "destroy")
                )
        }),
    )?;
    let (player, loaded, players_service) = {
        let world = ctx.borrow();
        (
            world
                .snapshot
                .get("local_player")
                .cloned()
                .unwrap_or(Json::Null),
            world.snapshot["initial_replication_complete"]
                .as_bool()
                .unwrap_or(false),
            world.services.get("Players").cloned(),
        )
    };
    if player != previous_player {
        if let Some(id) = players_service {
            emit_property_changed_direct(lua, ctx, &id, "LocalPlayer")?;
        }
    }
    if loaded && !previously_loaded {
        if let Some(id) = game {
            super::signals::fire(lua, ctx, &id, "Loaded", MultiValue::new())?;
        }
    }

    for (action_index, action) in actions.iter().enumerate() {
        let sequence = action.get("sequence").and_then(Json::as_u64).unwrap_or(0);
        if sequence <= starting_sequence {
            continue;
        }
        player_events.publish(lua, ctx, action_index)?;
        collection_events.publish(lua, ctx, action_index)?;
        attribute_events.publish(lua, ctx, action_index)?;
        // Received attribute writes expose the same `$Attribute*` child the
        // engine builds, so scripts walking children see the value.
        if let Some(update) = action.get("attribute_update") {
            if let (Some(id), Some(name)) = (
                action.get("id").and_then(Json::as_str),
                update.get("name").and_then(Json::as_str),
            ) {
                let value = if update.get("known").and_then(Json::as_bool) == Some(true) {
                    update.get("value")
                } else {
                    None
                };
                sync_attribute_child(lua, ctx, id, name, value)?;
            }
        }
        let destroyed = hierarchy_events.publish(lua, ctx, action_index)?;
        property_events.publish(lua, ctx, action_index)?;
        health_events.publish(lua, ctx, action_index)?;
        state_events.publish(lua, ctx, action_index)?;
        for id in &destroyed {
            cleanup_destroyed_bindings(lua, ctx, id);
        }
        destroyed_nodes.extend(destroyed);
    }
    super::movement::received_joint_poses(
        lua,
        ctx,
        actions,
        starting_sequence,
        &joint_registration,
    )?;
    let result = refresh_derived_properties(lua);
    for id in &destroyed_nodes {
        ctx.borrow_mut().nodes.compact_destroyed(id);
    }
    result
}

fn action_topology_fields(action: &Json) -> bool {
    action["fields"].as_object().is_some_and(|fields| {
        fields.keys().any(|key| joint_topology_property(key))
    }) || action["property"]
        .as_str()
        .or_else(|| action["key"].as_str())
        .is_some_and(joint_topology_property)
}

fn action_affects_actor_runnable(action: &Json) -> bool {
    if matches!(action["kind"].as_str(), Some("create" | "remove" | "destroy")) {
        return true;
    }
    const KEYS: &[&str] = &["Parent", "Disabled", "Enabled", "RunContext", "Source"];
    action["fields"].as_object().is_some_and(|fields| {
        fields.keys().any(|key| KEYS.contains(&key.as_str()))
    }) || action["property"]
        .as_str()
        .or_else(|| action["key"].as_str())
        .is_some_and(|key| KEYS.contains(&key))
}

fn joint_topology_property(key: &str) -> bool {
    matches!(
        key,
        "Parent" | "Anchored" | "Enabled" | "Part0" | "Part1" | "Attachment0"
            | "Attachment1" | "Mode" | "RelativeTo"
    )
}

/// A committed received property supersedes its script value and linked aliases.
fn clear_received_property_override(node: &mut Node, key: &str) {
    super::movement::clear_received_part_overrides(node, key);
    super::movement::clear_received_attachment_alias(node, key);
    if node.class_name == "Humanoid" && key == "HumanoidStateType" {
        node.local_overrides.remove("__HumanoidStateFeedback");
    }
    node.local_overrides.remove(key);
    if let Some(canonical) = super::world_aliases::canonical_key(&node.class_name, key) {
        node.local_overrides.remove(canonical);
    }
}

/// Refresh an Actor fork from the root after another VM committed local state,
/// publishing the corresponding instance signals in the receiving VM.
pub fn sync_actor_fork_with_lua(
    lua: &Lua,
    actor_world: &WorldRef,
    root_world: &WorldRef,
    journal: &[Json],
) -> mlua::Result<()> {
    let camera_before = super::camera::observe_actor(actor_world, root_world);
    let mut player_events = super::player_lifecycle::prepare(lua, actor_world, journal, None, None);
    let mut collection_events = super::collection_events::prepare(actor_world, journal, None);
    let mut attribute_events = super::attribute_events::prepare(actor_world, journal, None);
    let mut hierarchy_events = super::hierarchy_events::prepare(lua, actor_world, journal, None)?;
    let mut state_events = super::humanoid_state::prepare(actor_world, journal, None)?;
    let mut health_events = super::humanoid_health::prepare(actor_world, journal, None);
    let mut property_events = super::property_events::prepare(lua, actor_world, journal, None);
    {
        let root = root_world.borrow();
        let mut actor = actor_world.borrow_mut();
        // Prepare notifications against the old values above, then discard only
        // overlays superseded by accepted receipts. Local journals have no
        // sequence; stale receipts must not undo subsequent script writes.
        for row in journal {
            if !row["sequence"]
                .as_u64()
                .is_some_and(|s| s > actor.sequence && s <= root.sequence)
                || !super::property_events::relevant(row)
            {
                continue;
            }
            let Some(id) = row["id"].as_str() else {
                continue;
            };
            if !actor
                .nodes
                .peek(id)
                .is_some_and(|node| !node.local_overrides.is_empty())
            {
                continue;
            }
            let Some(node) = actor.nodes.get_mut(id) else {
                continue;
            };
            for key in super::property_events::fields(row).keys() {
                clear_received_property_override(node, key);
            }
        }
        actor.sync_actor_fork(&root);
    }
    // Actor sync may merge topology at the same replicated sequence. Evict
    // only when its committed journal includes a connectivity mutation.
    if journal.iter().any(|row| {
        matches!(row["kind"].as_str(), Some("create" | "destroy" | "remove"))
            || action_topology_fields(row)
    }) {
        super::joint_graph::invalidate_cache(actor_world);
    }
    player_events.commit();
    super::geometry::mass_events::synchronize(lua, actor_world, &property_events)?;
    resolve_instance_handle_waiters(lua, actor_world)?;
    super::camera::refresh_after_actor_sync(lua, actor_world, camera_before, journal)?;
    for (row_index, row) in journal.iter().enumerate() {
        player_events.publish(lua, actor_world, row_index)?;
        collection_events.publish(lua, actor_world, row_index)?;
        attribute_events.publish(lua, actor_world, row_index)?;
        for id in hierarchy_events.publish(lua, actor_world, row_index)? {
            cleanup_destroyed_bindings(lua, actor_world, &id);
        }
        property_events.publish(lua, actor_world, row_index)?;
        health_events.publish(lua, actor_world, row_index)?;
        state_events.publish(lua, actor_world, row_index)?;
        if row["kind"] == "curve_keys" {
            super::curves::replay_events(lua, actor_world, row)?;
        }
    }
    refresh_derived_properties(lua)
}

pub(super) fn fire_attribute_changed(
    lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    name: &str,
) -> mlua::Result<()> {
    let mut args = MultiValue::new();
    args.push_back(Value::String(lua.create_string(name)?));
    super::signals::fire(lua, ctx, id, "AttributeChanged", args)?;
    super::signals::fire(
        lua,
        ctx,
        id,
        &format!("attribute:{name}"),
        MultiValue::new(),
    )
}

/// Value-holder class the engine uses to expose an attribute as a child.
fn attribute_child_class(value: &Json) -> Option<&'static str> {
    if value.get("$vector2").is_some() {
        return Some("Vector2Value");
    }
    if value.get("$vector3").is_some() {
        return Some("Vector3Value");
    }
    if value.get("$color3").is_some() {
        return Some("Color3Value");
    }
    if value.get("$cframe").is_some() {
        return Some("CFrameValue");
    }
    if value.get("$brickcolor").is_some() {
        return Some("BrickColorValue");
    }
    if value.get("$ray").is_some() {
        return Some("RayValue");
    }
    if value.get("$instance_handle").is_some() || value.get("$ref").is_some() {
        return Some("ObjectValue");
    }
    match value {
        Json::Bool(_) => Some("BoolValue"),
        Json::Number(_) => Some("NumberValue"),
        Json::String(_) => Some("StringValue"),
        _ => None,
    }
}

/// The engine exposes every attribute as a `$Attribute<name>` child holding
/// the value. Games walk children directly instead of calling GetAttribute,
/// so keep these holders in sync.
pub(crate) fn sync_attribute_child(
    lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    name: &str,
    value: Option<&Json>,
) -> mlua::Result<()> {
    if ctx.borrow().nodes.get(id).is_none_or(|n| n.destroyed) {
        return Ok(());
    }
    let child_name = format!("$Attribute{name}");
    let existing = {
        let world = ctx.borrow();
        world.nodes.child_named(id, &child_name, &world.node_order)
    };
    let Some(value) = value.filter(|v| !v.is_null()) else {
        if let Some(child) = existing {
            destroy_instance(lua, ctx, &child)?;
        }
        return Ok(());
    };
    let Some(class) = attribute_child_class(value) else {
        return Ok(());
    };
    let encoded = match value.get("$instance_handle").and_then(Json::as_str) {
        Some(target) => json!({"$ref":target}),
        None => value.clone(),
    };
    match existing {
        Some(child) => {
            if ctx.borrow().nodes.get(&child).is_some_and(|n| n.class_name == class) {
                set_property(lua, ctx, &child, "Value", encoded)?;
            } else {
                destroy_instance(lua, ctx, &child)?;
                create_attribute_child(lua, ctx, id, &child_name, class, encoded)?;
            }
        }
        None => create_attribute_child(lua, ctx, id, &child_name, class, encoded)?,
    }
    Ok(())
}

fn create_attribute_child(
    lua: &Lua,
    ctx: &WorldRef,
    parent: &str,
    name: &str,
    class: &str,
    value: Json,
) -> mlua::Result<()> {
    let child = create_instance(lua, ctx, class, Some(parent))?;
    let child_id = instance_id(&child).ok_or_else(|| {
        mlua::Error::RuntimeError("attribute holder id missing".into())
    })?;
    let mut world = ctx.borrow_mut();
    if let Some(node) = world.nodes.get_mut(&child_id) {
        node.name = name.to_owned();
        node.local_overrides.insert("Value".into(), value);
    }
    Ok(())
}

pub fn register_method(ctx: &WorldRef, class: &'static str, name: &'static str, method: Method) {
    register_method_closure(ctx, class, name, method);
}
pub fn register_method_closure<F>(
    ctx: &WorldRef,
    class: &'static str,
    name: &'static str,
    method: F,
) where
    F: Fn(&Lua, &WorldRef, &str, MultiValue) -> mlua::Result<MultiValue> + 'static,
{
    ctx.borrow_mut()
        .methods
        .insert((class.into(), name.into()), Rc::new(method));
}
pub(crate) fn class_chain(class: &str) -> Vec<String> {
    let parents = class_parents();
    let mut result = Vec::new();
    let mut pending = vec![class.to_owned()];
    while let Some(current) = pending.pop() {
        if result.contains(&current) {
            continue;
        }
        result.push(current.clone());
        if result.len() >= 256 {
            break;
        }
        if let Some(ancestors) = parents.get(&current) {
            pending.extend(ancestors.iter().rev().cloned());
        }
    }
    if !result.iter().any(|name| name == "Instance") {
        result.push("Instance".into());
    }
    result
}
fn canonical_member(class: &str, key: &str) -> String {
    match key {
        "workspace" if class == "DataModel" => "Workspace",
        "className" => "ClassName",
        "archivable" => "Archivable",
        "children" | "getChildren" => "GetChildren",
        "clone" => "Clone",
        "destroy" => "Destroy",
        "remove" => "Remove",
        "findFirstChild" => "FindFirstChild",
        "waitForChild" => "WaitForChild",
        "isA" => "IsA",
        "isDescendantOf" => "IsDescendantOf",
        "childAdded" => "ChildAdded",
        "getFullName" => "GetFullName",
        "character" if class == "Player" => "Character",
        "TextWrap" if matches!(class, "TextLabel" | "TextButton" | "TextBox") => "TextWrapped",
        _ => key,
    }
    .into()
}
pub fn register_getter(ctx: &WorldRef, key: &'static str, getter: Getter) {
    ctx.borrow_mut()
        .getters
        .entry(key.into())
        .or_default()
        .push(getter);
}
pub fn register_keyed_getter(ctx: &WorldRef, key: &'static str, getter: KeyedGetter) {
    ctx.borrow_mut()
        .keyed_getters
        .entry(key.into())
        .or_default()
        .push(getter);
}
pub fn register_setter(ctx: &WorldRef, key: &'static str, setter: Setter) {
    ctx.borrow_mut()
        .setters
        .entry(key.into())
        .or_default()
        .push(setter);
}
pub fn register_keyed_setter(ctx: &WorldRef, key: &'static str, setter: KeyedSetter) {
    ctx.borrow_mut()
        .keyed_setters
        .entry(key.into())
        .or_default()
        .push(setter);
}

struct InstanceCacheTables(RefCell<HashMap<usize, (Weak<RefCell<World>>, mlua::Table)>>);

pub(crate) fn instance_cache_table(lua: &Lua, ctx: &WorldRef) -> mlua::Result<mlua::Table> {
    let world_key = Rc::as_ptr(ctx) as usize;
    if let Some(caches) = lua.app_data_ref::<InstanceCacheTables>() {
        let mut caches = caches.0.borrow_mut();
        caches.retain(|_, (world, _)| world.strong_count() > 0);
        if let Some((world, cache)) = caches.get(&world_key) {
            if Weak::ptr_eq(world, &Rc::downgrade(ctx)) {
                // The weak cache is a real VM table. Make the same table
                // discoverable through Luau's registry so `getreg()` reflects
                // live cached wrappers; weak values ensure registry visibility
                // does not keep Instances alive by itself.
                expose_instance_cache(lua, world_key, cache)?;
                return Ok(cache.clone());
            }
        }
        caches.remove(&world_key);
    }
    let cache = lua.create_table()?;
    let metatable = lua.create_table()?;
    metatable.set("__mode", "v")?;
    cache.set_metatable(Some(metatable))?;
    if lua.app_data_ref::<InstanceCacheTables>().is_none() {
        lua.set_app_data(InstanceCacheTables(RefCell::new(HashMap::new())));
    }
    if let Some(caches) = lua.app_data_ref::<InstanceCacheTables>() {
        let mut caches = caches.0.borrow_mut();
        caches.retain(|_, (world, _)| world.strong_count() > 0);
        caches.insert(world_key, (Rc::downgrade(ctx), cache.clone()));
    }
    expose_instance_cache(lua, world_key, &cache)?;
    Ok(cache)
}

fn expose_instance_cache(lua: &Lua, world_key: usize, cache: &mlua::Table) -> mlua::Result<()> {
    unsafe {
        lua.exec_raw::<()>(cache.clone(), |state| {
            mlua::ffi::lua_pushlightuserdata(state, world_key as *mut std::ffi::c_void);
            mlua::ffi::lua_pushvalue(state, -2);
            mlua::ffi::lua_rawset(state, mlua::ffi::LUA_REGISTRYINDEX);
        })
    }
}

pub fn instance(lua: &Lua, ctx: &WorldRef, id: &str) -> mlua::Result<Value> {
    let cache = instance_cache_table(lua, ctx)?;
    if let Value::UserData(value) = cache.raw_get::<Value>(id)? {
        // The cache is private to this World bucket, and cache.replace only
        // accepts handles from this World. Avoid borrowing the userdata here:
        // property setters can re-enter instance() while that same userdata is
        // mutably borrowed, and evicting it would change canonical identity.
        return Ok(Value::UserData(value));
    }
    if !ctx.borrow().nodes.contains_key(id) {
        return Ok(Value::Nil);
    }
    let userdata = lua.create_userdata(InstanceHandle {
        id: id.into(),
        world: Rc::downgrade(ctx),
    })?;
    cache.raw_set(id, userdata.clone())?;
    Ok(Value::UserData(userdata))
}

/// Make a second userdata wrapper for the same underlying Instance handle.
pub fn clone_instance_reference(lua: &Lua, value: &Value) -> mlua::Result<Value> {
    let Value::UserData(value) = value else {
        return Err(mlua::Error::RuntimeError("expected Instance".into()));
    };
    let handle = value
        .borrow::<InstanceHandle>()
        .map_err(|_| mlua::Error::RuntimeError("expected Instance".into()))?;
    Ok(Value::UserData(lua.create_userdata(InstanceHandle {
        id: handle.id.clone(),
        world: handle.world.clone(),
    })?))
}

/// Compare wrapper identity by underlying World and node, matching the native
/// handle comparison even when the world itself has since been released.
pub fn compare_instance_references(a: &Value, b: &Value) -> mlua::Result<bool> {
    let (Value::UserData(a), Value::UserData(b)) = (a, b) else {
        return Err(mlua::Error::RuntimeError("expected two Instances".into()));
    };
    let a = a
        .borrow::<InstanceHandle>()
        .map_err(|_| mlua::Error::RuntimeError("expected two Instances".into()))?;
    let b = b
        .borrow::<InstanceHandle>()
        .map_err(|_| mlua::Error::RuntimeError("expected two Instances".into()))?;
    Ok(a.id == b.id && Weak::ptr_eq(&a.world, &b.world))
}

pub fn cached_instance_ids(lua: &Lua, ctx: &WorldRef) -> mlua::Result<Vec<String>> {
    let cache = instance_cache_table(lua, ctx)?;
    let mut ids = Vec::new();
    for pair in cache.pairs::<String, Value>() {
        let (id, value) = pair?;
        if instance_belongs_to(&value, ctx) {
            ids.push(id);
        }
    }
    Ok(ids)
}

pub fn instance_id(value: &Value) -> Option<String> {
    let Value::UserData(data) = value else {
        return None;
    };
    data.borrow::<InstanceHandle>().ok().map(|h| h.id.clone())
}

/// True only when this userdata is one of the Instance handles owned by `ctx`.
pub fn instance_belongs_to(value: &Value, ctx: &WorldRef) -> bool {
    let Value::UserData(data) = value else {
        return false;
    };
    let Ok(handle) = data.borrow::<InstanceHandle>() else {
        return false;
    };
    handle
        .world
        .upgrade()
        .is_some_and(|world| Rc::ptr_eq(&world, ctx))
}

fn decode_instance_ref(lua: &Lua, value: &Json) -> mlua::Result<Value> {
    let id = value.get("$ref").and_then(Json::as_str).ok_or_else(|| {
        mlua::Error::RuntimeError("instance reference must contain a string $ref".into())
    })?;
    let weak = lua.app_data_ref::<Weak<RefCell<World>>>().ok_or_else(|| {
        mlua::Error::RuntimeError("no world is installed in this Luau state".into())
    })?;
    let ctx = weak
        .upgrade()
        .ok_or_else(|| mlua::Error::RuntimeError("world has been released".into()))?;
    instance(lua, &ctx, id)
}

/// Decode an Instance property payload using the exact handles from this
/// World. Unlike the generic value codec, this also understands the snapshot
/// reference and TextChatMessage status tags.
pub fn value_from_json(lua: &Lua, ctx: &WorldRef, value: &Json) -> mlua::Result<Value> {
    if value.get("$ref").is_some() {
        let id = value
            .get("$ref")
            .and_then(Json::as_str)
            .ok_or_else(|| mlua::Error::RuntimeError("invalid Instance reference".into()))?;
        return instance(lua, ctx, id);
    }
    if value.get("$instance_handle").is_some() {
        return decode_instance_handle(lua, value);
    }
    if let Some(status) = value.get("$chat_status").and_then(Json::as_i64) {
        let item = match status {
            1 => "Unknown",
            2 => "Success",
            3 => "Sending",
            4 => "TextFilterFailed",
            5 => "Floodchecked",
            6 => "InvalidPrivacySettings",
            7 => "InvalidTextChannelPermissions",
            8 => "MessageTooLong",
            9 => "ModerationTimeout",
            _ => {
                return Err(mlua::Error::RuntimeError(
                    "unsupported TextChatMessageStatus".into(),
                ));
            }
        };
        return Ok(Value::UserData(super::enums::enum_item(
            lua,
            "TextChatMessageStatus",
            item,
        )?));
    }
    if let Some(tag) = value.get("$enum") {
        let enum_name = tag
            .get("type")
            .and_then(Json::as_str)
            .ok_or_else(|| mlua::Error::RuntimeError("$enum.type must be a string".into()))?;
        let expected = tag
            .get("value")
            .and_then(Json::as_i64)
            .ok_or_else(|| mlua::Error::RuntimeError("$enum.value must be an integer".into()))?;
        let enum_root: mlua::Table = lua.globals().get("Enum")?;
        let enum_type: AnyUserData = enum_root.get(enum_name)?;
        let item = if let Some(name) = tag.get("name").and_then(Json::as_str) {
            enum_type.get::<Value>(name)?
        } else {
            let from_value: mlua::Function = enum_type.get("FromValue")?;
            from_value.call::<Value>(expected as f64)?
        };
        let Value::UserData(item) = item else {
            return Err(mlua::Error::RuntimeError("unknown $enum value".into()));
        };
        if !super::enums::enum_item_matches(&item, enum_name)?
            || super::enums::enum_item_value(&item)? as i64 != expected
        {
            return Err(mlua::Error::RuntimeError(
                "$enum name/value mismatch".into(),
            ));
        }
        return Ok(Value::UserData(item));
    }
    super::values::from_json(lua, value)
}

fn encode_instance_ref(data: &AnyUserData) -> mlua::Result<Option<Json>> {
    let Ok(handle) = data.borrow::<InstanceHandle>() else {
        return Ok(None);
    };
    if handle.world.upgrade().is_none() {
        return Ok(None);
    }
    Ok(Some(json!({"$ref":handle.id})))
}

struct InstanceRefValue {
    id: String,
    world: Weak<RefCell<World>>,
}
impl UserData for InstanceRefValue {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field("__type", "InstanceHandle");
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Index, |lua, _this, key: String| {
            match key.as_str() {
                "Get" => {
                    Ok(Value::Function(lua.create_function(move |lua, receiver: AnyUserData| {
                        let (id, owner) = instance_handle_receiver(&receiver)?;
                        let world = owner.upgrade().ok_or_else(|| mlua::Error::RuntimeError("world has been released".into()))?;
                        if id.is_empty() { Ok(Value::Nil) } else { instance(lua, &world, &id) }
                    })?))
                }
                "Wait" => {
                    let register = lua.create_function(move |lua, (receiver, timeout): (AnyUserData, Option<f64>)| {
                        let (id, owner) = instance_handle_receiver(&receiver)?;
                        begin_instance_handle_wait(lua, &owner, &id, timeout)
                    })?;
                    let wrapper: mlua::Function = lua.load(
                        "return function(register) return function(self, timeout)\n\
                         local ok, done, value = pcall(register, self, timeout)\n\
                         if not ok then error(tostring(done), 2) end\n\
                         if done then return value end\n\
                         if not coroutine.isyieldable() then error('waiting for an unresolved InstanceHandle requires a scheduled yieldable thread', 2) end\n\
                         return coroutine.yield()\n\
                         end end",
                    ).eval()?;
                    Ok(Value::Function(wrapper.call::<mlua::Function>(register)?))
                }
                _ => Err(mlua::Error::RuntimeError("unknown InstanceHandle member".into())),
            }
        });
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: Value| {
            let Value::UserData(other) = other else {
                return Ok(false);
            };
            let Ok(other) = other.borrow::<InstanceRefValue>() else {
                return Ok(false);
            };
            let same_world = match (this.world.upgrade(), other.world.upgrade()) {
                (Some(a), Some(b)) => Rc::ptr_eq(&a, &b),
                _ => false,
            };
            Ok(same_world && this.id == other.id)
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            if this.id.is_empty() {
                return Ok("nil".to_owned());
            }
            let Some(world) = this.world.upgrade() else {
                return Ok("nil".to_owned());
            };
            let name = world
                .borrow()
                .nodes
                .get(&this.id)
                .map(|node| node.name.clone())
                .unwrap_or_else(|| "nil".into());
            Ok(name)
        });
    }
}

fn instance_handle_receiver(
    receiver: &AnyUserData,
) -> mlua::Result<(String, Weak<RefCell<World>>)> {
    let handle = receiver
        .borrow::<InstanceRefValue>()
        .map_err(|_| mlua::Error::RuntimeError("InstanceHandle receiver required".into()))?;
    Ok((handle.id.clone(), handle.world.clone()))
}

fn begin_instance_handle_wait(
    lua: &Lua,
    weak_world: &Weak<RefCell<World>>,
    id: &str,
    timeout: Option<f64>,
) -> mlua::Result<(bool, Value)> {
    begin_instance_wait(lua, weak_world, id, None, timeout)
}

fn wait_target(ctx: &WorldRef, id: &str, child_name: Option<&str>) -> Option<String> {
    let world = ctx.borrow();
    if let Some(name) = child_name {
        if !world.nodes.get(id).is_some_and(|node| !node.destroyed) {
            return None;
        }
        world.nodes.child_named(id, name, &world.node_order)
    } else {
        world.nodes.contains_key(id).then(|| id.to_owned())
    }
}

fn begin_instance_wait(
    lua: &Lua,
    weak_world: &Weak<RefCell<World>>,
    id: &str,
    child_name: Option<String>,
    timeout: Option<f64>,
) -> mlua::Result<(bool, Value)> {
    let operation = if child_name.is_some() {
        "WaitForChild"
    } else {
        "InstanceHandle"
    };
    if timeout.is_some_and(|seconds| {
        !seconds.is_finite() || seconds < 0.0 || (seconds == 0.0 && child_name.is_none())
    }) {
        return Err(mlua::Error::RuntimeError(
            if child_name.is_some() {
                "timeOut must be finite and non-negative"
            } else {
                "timeOut must be greater than 0"
            }
            .into(),
        ));
    }
    let world = weak_world
        .upgrade()
        .ok_or_else(|| mlua::Error::RuntimeError("world has been released".into()))?;
    if id.is_empty() {
        return Ok((true, Value::Nil));
    }
    if let Some(target) = wait_target(&world, id, child_name.as_deref()) {
        return Ok((true, instance(lua, &world, &target)?));
    }
    if timeout == Some(0.0)
        || (child_name.is_some()
            && world
                .borrow()
                .nodes
                .get(id)
                .is_none_or(|node| node.destroyed))
    {
        return Ok((true, Value::Nil));
    }
    let waiters = lua
        .app_data_ref::<InstanceHandleWaiters>()
        .ok_or_else(|| {
            mlua::Error::RuntimeError(format!("{operation} requires a scheduled yieldable thread"))
        })?
        .clone();
    if unsafe { mlua::ffi::lua_isyieldable(lua.state()) } == 0 {
        return Err(mlua::Error::RuntimeError(format!(
            "{operation} requires a scheduled yieldable thread"
        )));
    }
    resolve_instance_handle_waiters(lua, &world)?;
    let (token, thread) = {
        let mut state = waiters.borrow_mut();
        if state.pending.len() >= 2048 {
            return Err(mlua::Error::RuntimeError(format!(
                "{operation} waiter limit exceeded"
            )));
        }
        state.next_id = state.next_id.wrapping_add(1).max(1);
        (state.next_id, lua.current_thread())
    };
    let timer = if let Some(seconds) = timeout {
        let callback = lua.create_function(move |lua, ()| {
            timeout_instance_handle_waiter(lua, token)?;
            Ok(())
        })?;
        let runner: mlua::Function = lua.load("return function(seconds, callback) local remaining=seconds; while remaining>86400 do task.wait(86400); remaining=remaining-86400 end; if remaining>0 then task.wait(remaining) end; callback() end").eval()?;
        let args = MultiValue::from_vec(vec![Value::Number(seconds), Value::Function(callback)]);
        Some(super::scheduler::enqueue(lua, runner, args)?)
    } else {
        None
    };
    if let Err(error) = super::scheduler::park(lua, &thread) {
        if let Some(timer) = timer {
            cancel_instance_handle_timer(lua, timer);
        }
        return Err(mlua::Error::RuntimeError(format!(
            "{operation} requires a scheduled yieldable thread: {error}"
        )));
    }
    waiters.borrow_mut().pending.push(InstanceHandleWaiter {
        token,
        id: id.to_owned(),
        child_name,
        world: weak_world.clone(),
        thread,
        started: Instant::now(),
        timeout_seconds: timeout,
        timer,
    });
    Ok((false, Value::Nil))
}

fn cancel_instance_handle_timer(lua: &Lua, thread: Thread) {
    let result = (|| -> mlua::Result<()> {
        let task: mlua::Table = lua.globals().get("task")?;
        let cancel: mlua::Function = task.get("cancel")?;
        cancel.call::<()>(thread)
    })();
    let _ = result;
}

fn timeout_instance_handle_waiter(lua: &Lua, token: u64) -> mlua::Result<()> {
    let waiters = lua
        .app_data_ref::<InstanceHandleWaiters>()
        .ok_or_else(|| {
            mlua::Error::RuntimeError("InstanceHandle wait state is unavailable".into())
        })?
        .clone();
    let pending = {
        let mut state = waiters.borrow_mut();
        let Some(index) = state
            .pending
            .iter()
            .position(|waiter| waiter.token == token)
        else {
            return Ok(());
        };
        let expired = state.pending[index]
            .timeout_seconds
            .is_some_and(|seconds| state.pending[index].started.elapsed().as_secs_f64() >= seconds);
        if !expired {
            return Ok(());
        }
        state.pending.remove(index)
    };
    if super::scheduler::is_waiting(lua, &pending.thread) {
        super::scheduler::resume(lua, pending.thread, MultiValue::from_vec(vec![Value::Nil]))?;
    }
    Ok(())
}

pub(super) fn resolve_instance_handle_waiters(lua: &Lua, ctx: &WorldRef) -> mlua::Result<()> {
    let Some(waiters) = lua
        .app_data_ref::<InstanceHandleWaiters>()
        .map(|state| state.clone())
    else {
        return Ok(());
    };
    let ready = {
        let mut state = waiters.borrow_mut();
        let mut ready = Vec::new();
        let mut i = 0;
        while i < state.pending.len() {
            let waiter = &state.pending[i];
            let same_world = waiter
                .world
                .upgrade()
                .is_some_and(|world| Rc::ptr_eq(&world, ctx));
            if !same_world {
                i += 1;
                continue;
            }
            let target = wait_target(ctx, &waiter.id, waiter.child_name.as_deref());
            let destroyed_parent = waiter.child_name.is_some()
                && ctx
                    .borrow()
                    .nodes
                    .get(&waiter.id)
                    .is_none_or(|node| node.destroyed);
            let expired = waiter
                .timeout_seconds
                .is_some_and(|seconds| waiter.started.elapsed().as_secs_f64() >= seconds);
            if target.is_some()
                || waiter.id.is_empty()
                || destroyed_parent
                || expired
                || !super::scheduler::is_waiting(lua, &waiter.thread)
            {
                ready.push((state.pending.remove(i), target));
            } else {
                i += 1;
            }
        }
        ready
    };
    for (waiter, target) in ready {
        if let Some(timer) = waiter.timer {
            cancel_instance_handle_timer(lua, timer);
        }
        if !super::scheduler::is_waiting(lua, &waiter.thread) {
            continue;
        }
        let value = if let Some(target) = target {
            instance(lua, ctx, &target)?
        } else {
            Value::Nil
        };
        super::scheduler::resume(lua, waiter.thread, MultiValue::from_vec(vec![value]))?;
    }
    Ok(())
}

struct ChildWaitFunction(mlua::Function);
fn child_wait_function(lua: &Lua) -> mlua::Result<mlua::Function> {
    if let Some(function) = lua.app_data_ref::<ChildWaitFunction>() {
        return Ok(function.0.clone());
    }
    let register = lua.create_function(
        |lua, (receiver, name, timeout): (AnyUserData, String, Option<f64>)| {
            let handle = receiver.borrow::<InstanceHandle>().map_err(|_| {
                mlua::Error::RuntimeError("WaitForChild requires an Instance receiver".into())
            })?;
            begin_instance_wait(lua, &handle.world, &handle.id, Some(name), timeout)
        },
    )?;
    let wrapper: mlua::Function = lua
        .load(
            r#"
        local call, raise, yield = pcall, error, coroutine.yield
        return function(register)
            return function(self, name, timeout)
                local ok, done, value = call(register, self, name, timeout)
                if not ok then raise(done, 2) end
                if done then return value end
                return yield()
            end
        end
    "#,
        )
        .eval()?;
    let function = wrapper.call::<mlua::Function>(register)?;
    lua.set_app_data(ChildWaitFunction(function.clone()));
    Ok(function)
}
fn decode_instance_handle(lua: &Lua, value: &Json) -> mlua::Result<Value> {
    let id = match value.get("$instance_handle") {
        Some(Json::Null) => String::new(),
        Some(Json::String(id)) => id.clone(),
        _ => {
            return Err(mlua::Error::RuntimeError(
                "instance handle must contain a string id or null".into(),
            ));
        }
    };
    let weak = lua.app_data_ref::<Weak<RefCell<World>>>().ok_or_else(|| {
        mlua::Error::RuntimeError("no world is installed in this Luau state".into())
    })?;
    if weak.upgrade().is_none() {
        return Err(mlua::Error::RuntimeError("world has been released".into()));
    }
    Ok(Value::UserData(lua.create_userdata(InstanceRefValue {
        id,
        world: weak.clone(),
    })?))
}
fn encode_instance_handle(data: &AnyUserData) -> mlua::Result<Option<Json>> {
    let Ok(handle) = data.borrow::<InstanceRefValue>() else {
        return Ok(None);
    };
    if handle.world.upgrade().is_none() {
        return Ok(None);
    }
    Ok(Some(if handle.id.is_empty() {
        json!({"$instance_handle":null})
    } else {
        json!({"$instance_handle":handle.id})
    }))
}

pub fn get_property(ctx: &WorldRef, id: &str, key: &str) -> mlua::Result<Json> {
    get_property_from_world(&ctx.borrow(), id, key)
}

pub(crate) fn received_creation_complete(ctx: &WorldRef, id: &str) -> bool {
    ctx.borrow()
        .nodes
        .get(id)
        .is_some_and(|node| node.received_creation_complete)
}

/// Find a Workspace child Model whose name matches `player_name`.
fn character_named_for(world: &World, player_name: &str) -> Option<String> {
    let mut found = None;
    for node in world.nodes.values() {
        if found.is_none()
            && node.class_name == "Model"
            && node.name == player_name
            && node
                .parent
                .as_deref()
                .and_then(|parent| world.nodes.class_name(parent))
                == Some("Workspace")
        {
            found = Some(node.id.to_string());
        }
    }
    found
}

pub(crate) fn get_property_from_world(world: &World, id: &str, key: &str) -> mlua::Result<Json> {
    if key == "BrickColor"
        && world.nodes.class_name(id).is_some_and(|class| {
            class == "BasePart" || class_chain(class).iter().any(|c| c == "BasePart")
        })
    {
        if let Ok(color) = get_property_from_world(world, id, "Color") {
            return super::sequences::closest_brick_json(&color);
        }
    }
    let node = world
        .nodes
        .get(id)
        .ok_or_else(|| mlua::Error::RuntimeError("instance is not in this world".into()))?;
    // The join response owns this ID; ordinary world replication omits it.
    if node.class_name == "DataModel" && key == "JobId" {
        if let Some(value) = world.snapshot.get("job_id") {
            return value
                .as_str()
                .map(|job_id| json!(job_id))
                .ok_or_else(|| mlua::Error::RuntimeError("session JobId must be a string".into()));
        }
    }
    if key == "CFrame"
        && class_chain(&node.class_name)
            .iter()
            .any(|c| c == "Attachment")
    {
        return super::movement::attachment_frame(node);
    }
    // Streaming servers often create the character model before setting the
    // Player.Character reference. Resolve it by name so scripts and the
    // prediction pipeline see the character as soon as it exists.
    if key == "Character"
        && node.class_name == "Player"
        && node.properties.get("Character").is_none()
        && node.local_overrides.get("Character").is_none()
    {
        if let Some(character) = character_named_for(world, &node.name) {
            return Ok(json!({"$ref": character}));
        }
    }
    if let Some(value) = node.local_overrides.get(key) {
        return Ok(value.clone());
    }
    if let Some(value) = super::movement::received_joint_property(node, key) {
        return Ok(value);
    }
    if let Some(value) = super::movement::part_transform_alias(node, key) {
        return value;
    }
    match key {
        "Name" => return Ok(json!(node.name)),
        "ClassName" => return Ok(json!(node.class_name)),
        "Parent" => {
            return Ok(node
                .parent
                .as_ref()
                .map(|p| json!({"$ref":p}))
                .unwrap_or(Json::Null));
        }
        _ => {}
    }
    if let Some(projected) = &node.projected {
        let received = projected.property(key).map_err(mlua::Error::RuntimeError)?;
        if let Some(value) = received {
            return Ok(value);
        }
        // An omitted pivot offset on a completed received creation keeps its
        // pinned constructor value. Incomplete snapshots stay unknown.
        if key == "PivotOffset"
            && projected.raw().record_field("received_creation_complete") == Some(json!(true))
        {
            if let Some(value) = constructor_class(&node.class_name)
                .and_then(|class| class.get("properties"))
                .and_then(|properties| properties.get(key))
                .and_then(|property| property.get("value"))
            {
                return Ok(value.clone());
            }
        }
        // Player.Character is a nullable engine property that begins nil,
        // but Player is intentionally absent from the creatable-class
        // constructor catalog. Use that default only when the complete
        // received creation confirms the property was omitted; explicit
        // received values above always take precedence.
        if node.class_name == "Player"
            && key == "Character"
            && projected.raw().record_field("received_creation_complete") == Some(json!(true))
        {
            return Ok(Json::Null);
        }
        if let Some(wire_key) = super::world_aliases::wire_key(&node.class_name, key) {
            if let Some(value) = projected
                .property(wire_key)
                .map_err(mlua::Error::RuntimeError)?
            {
                return super::world_aliases::from_wire(&node.class_name, wire_key, &value)
                    .map_err(mlua::Error::RuntimeError)?
                    .ok_or_else(|| {
                        mlua::Error::RuntimeError(format!(
                            "property '{key}' unavailable in snapshot for {} ({})",
                            node.class_name, node.class_name
                        ))
                    });
            }
        }
        // A completed received creation omits values that still have pinned
        // engine defaults. Use the catalog only in this case; partial captures
        // must keep omitted values unknown. The explicit/wire values above win.
        if projected.raw().record_field("received_creation_complete") == Some(json!(true)) {
            if let Some(value) = player_constructor_default(&node.class_name, key) {
                return Ok(value);
            }
            if let Ok((value, _)) = constructor_default(&node.class_name, key) {
                return Ok(value);
            }
        }
        // The projected (live replication) path always knows the node's
        // class, so value-holder defaults are safe even for partial creates;
        // a reader that indexes `.X` must never see nil.
        if let Some(value) = client_omitted_default(&node.class_name, key, true, false) {
            return Ok(value);
        }
        if world.lightweight_assets && super::world_aliases::is_lightweight_asset(key) {
            return Err(mlua::Error::RuntimeError(format!(
                "property '{key}' unavailable in lightweight asset mode"
            )));
        }
        return Err(mlua::Error::RuntimeError(format!(
            "property '{key}' unavailable in snapshot for {} ({})",
            node.class_name, node.class_name
        )));
    }
    if let Some(value) = node.properties.get(key) {
        return Ok(value.clone());
    }
    if let Some(wire_key) = super::world_aliases::wire_key(&node.class_name, key) {
        if let Some(value) = node.properties.get(wire_key) {
            return super::world_aliases::from_wire(&node.class_name, wire_key, value)
                .map_err(mlua::Error::RuntimeError)?
                .ok_or_else(|| {
                    mlua::Error::RuntimeError(format!(
                        "property '{key}' unavailable in snapshot for {} ({})",
                        node.class_name, node.class_name
                    ))
                });
        }
    }
    if world.lightweight_assets && super::world_aliases::is_lightweight_asset(key) {
        return Err(mlua::Error::RuntimeError(format!(
            "property '{key}' unavailable in lightweight asset mode"
        )));
    }
    if !node.server_owned {
        if let Some(default) = super::joint_graph::local_mode_default(&node.class_name, key) {
            return Ok(default);
        }
        if let Some(default) = local_part_mass_default(&node.class_name, key) {
            return Ok(default);
        }
    }
    if let Some(default) =
        client_omitted_default(&node.class_name, key, node.received_creation_complete, !node.server_owned)
    {
        return Ok(default);
    }
    Err(mlua::Error::RuntimeError(format!(
        "property '{key}' unavailable in snapshot for {} ({})",
        node.class_name, node.class_name
    )))
}

/// Engine defaults for client-side properties that replication omits.
fn client_omitted_default(class: &str, key: &str, complete: bool, local: bool) -> Option<Json> {
    // Local value holders (`$AttributeFoo` children) always need their typed
    // empty `Value`; replicated ones only once the creation completes, so a
    // partial capture still reports the property as unknown.
    // Value holders always answer their typed empty `Value`.
    if let Some(default) = value_object_default(class, key) {
        return Some(default);
    }
    // InputBinding is client-built by PlayerModule; replicated copies omit
    // most defaults (scale vectors, thresholds, enum directions).
    if class == "InputBinding" {
        return input_binding_default(key);
    }
    if class == "DataModel" {
        return match key {
            "CreatorType" => Some(json!({"$enum":{"type":"CreatorType","name":"User","value":0}})),
            "CreatorId" => Some(json!(0)),
            "Genre" => Some(json!({"$enum":{"type":"Genre","name":"All","value":1}})),
            _ => None,
        };
    }
    if class == "Terrain" {
        return match key {
            "Size" => Some(json!({"$vector3":[2048, 512, 2048]})),
            "Position" => Some(json!({"$vector3":[0, 0, 0]})),
            "CFrame" => Some(json!({"$cframe":{"position":[0,0,0],"rotation":[1,0,0,0,1,0,0,0,1]}})),
            "PivotOffset" => Some(json!({"$cframe":{"position":[0,0,0],"rotation":[1,0,0,0,1,0,0,0,1]}})),
            "WaterColor" => Some(json!({"$color3":[0.12,0.5,0.9]})),
            "WaterWaveSize" | "WaterWaveSpeed" => Some(json!(0)),
            "WaterTransparency" => Some(json!(0.1)),
            "Decoration" => Some(json!(true)),
            "MaxExtents" => Some(json!({"$vector3":[2048,512,2048]})),
            _ => None,
        };
    }
    if class == "InputAction" {
        return match key {
            "Enabled" | "Editor" => Some(json!(true)),
            "DisplayName" => Some(json!("")),
            "Type" => Some(json!({"$enum":{"type":"InputActionType","name":"Bool","value":0}})),
            "PreferredBinding" => Some(Json::Null),
            "BoolState" => Some(json!(false)),
            "Direction1DState" => Some(json!(0)),
            "Direction2DState" | "ViewportPositionState" => Some(json!({"$vector2":[0,0]})),
            "Direction3DState" => Some(json!({"$vector3":[0,0,0]})),
            _ => None,
        };
    }
    if class == "InputContext" {
        return match key {
            "Enabled" => Some(json!(true)),
            "Priority" => Some(json!(1000)),
            "Sink" | "Editor" => Some(json!(false)),
            _ => None,
        };
    }
    // GUI layout defaults live in the GuiObject/GuiBase2d base classes and are
    // omitted for freshly created UI. Streaming servers therefore hand scripts
    // Frames without Position/AnchorPoint/SizeConstraint, which used to break
    // every responsive layout. Supply the engine defaults.
    if class_chain(class).iter().any(|c| c == "GuiObject") {
        if let Some(default) = gui_object_default(key) {
            return Some(default);
        }
    }
    if class_chain(class).iter().any(|c| c == "GuiBase2d") {
        return gui_base_default(key);
    }
    None
}

/// Empty `Value` for each replicable value-holder class.
fn value_object_default(class: &str, key: &str) -> Option<Json> {
    if key != "Value" {
        return None;
    }
    let value = match class {
        "NumberValue" | "IntValue" | "FloatValue" | "DoubleConstrainedValue"
        | "IntConstrainedValue" => json!(0),
        "StringValue" => json!(""),
        "BoolValue" => json!(false),
        "Vector3Value" => json!({"$vector3":[0,0,0]}),
        "Vector2Value" => json!({"$vector2":[0,0]}),
        "Color3Value" => json!({"$color3":[0,0,0]}),
        "CFrameValue" => {
            json!({"$cframe":{"position":[0,0,0],"rotation":[1,0,0,0,1,0,0,0,1]}})
        }
        "BrickColorValue" => json!({"$brickcolor":194}),
        "RayValue" => json!({"$ray":[{"$vector3":[0,0,0]},{"$vector3":[0,0,-1]}]}),
        "ObjectValue" => Json::Null,
        _ => return None,
    };
    Some(value)
}

/// Client-owned `InputBinding` defaults that replication omits.
fn input_binding_default(key: &str) -> Option<Json> {
    let key_enum = |name: &str| json!({"$enum":{"type":"KeyCode","name":name,"value":0}});
    let value = match key {
        "Vector2Scale" => json!({"$vector2":[1,1]}),
        "Vector3Scale" => json!({"$vector3":[1,1,1]}),
        "PressedThreshold" => json!(0.5),
        "ReleasedThreshold" => json!(0.1),
        "ResponseCurve" => json!(1),
        "Scale" => json!(1),
        "PointerIndex" => json!(1),
        "ClampMagnitudeToOne" => json!(true),
        "DisplayName" => json!(""),
        "Type" => json!({"$enum":{"type":"InputBindingType","name":"Automatic","value":0}}),
        "UIButton" | "UIModifier" => Json::Null,
        "Forward" | "Backward" | "Left" | "Right" | "Up" | "Down" => key_enum("None"),
        "KeyCode" | "PrimaryModifier" | "SecondaryModifier" => key_enum("None"),
        _ => return None,
    };
    Some(value)
}

fn gui_base_default(key: &str) -> Option<Json> {
    let value = match key {
        "Rotation" => json!(0),
        "Localize" | "AutoLocalize" => json!(true),
        "SelectionOrder" | "SelectionGroup" => json!(0),
        "SelectionImageObject" | "RootLocalizationTable" => Json::Null,
        "Archivable" | "archivable" => json!(true),
        _ => return None,
    };
    Some(value)
}

/// Inherited `GuiBase2d` defaults that replication omits.
fn gui_base_default_placeholder() {}

/// Inherited `GuiObject` defaults that replication omits. Values match the
/// engine constructor; `Position`/`Size` default to zero UDim2 and
/// `AnchorPoint` to zero Vector2, which is what responsive layouts expect.
fn gui_object_default(key: &str) -> Option<Json> {
    let value = match key {
        "Position" => json!({"$udim2":[{"$udim":[0,0]},{"$udim":[0,0]}]}),
        "Size" => json!({"$udim2":[{"$udim":[0,0]},{"$udim":[0,0]}]}),
        "AnchorPoint" => json!({"$vector2":[0,0]}),
        "BackgroundColor3" => json!({"$color3":[1,1,1]}),
        "BackgroundTransparency" | "Transparency" => json!(0),
        "BorderColor3" => json!({"$color3":[0,0,0]}),
        "BorderSizePixel" => json!(1),
        "SizeConstraint" => json!({"$enum":{"type":"SizeConstraint","name":"RelativeXY","value":0}}),
        "AutomaticSize" => json!({"$enum":{"type":"AutomaticSize","name":"None","value":0}}),
        "ClipsDescendants" | "Draggable" | "Selectable" | "Active" | "SelectionGroup" => json!(false),
        "Interactable" => json!(true),
        "ZIndex" => json!(1),
        "LayoutOrder" => json!(0),
        "Visible" => json!(true),
        "NextSelectionUp" | "NextSelectionDown" | "NextSelectionLeft" | "NextSelectionRight" => {
            Json::Null
        }
        _ => return None,
    };
    Some(value)
}

fn archivable_class_default(class: &str) -> Option<Json> {
    constructor_default(class, "Archivable")
        .ok()
        .map(|(value, _)| value)
        .filter(Json::is_boolean)
}

// Match Python's completed-create adapter. An omitted property on a complete
// received creation retains the pinned constructor value; partial captures do
// not. Use the wire spelling so later received writes replace this fallback.
fn received_complete_create_defaults(
    record: &JsonMap<String, Json>,
    class: &str,
    properties: &mut JsonMap<String, Json>,
) {
    if record.get("received_creation_complete") != Some(&json!(true)) {
        return;
    }
    // Roblox Player.Character starts as nil. Keep this tied to complete
    // creations so incomplete captures do not turn missing state into nil.
    if class == "Player" && !properties.contains_key("Character") {
        properties.insert("Character".into(), Json::Null);
    }
    if class == "Player" {
        for (key, value) in player_constructor_defaults() {
            properties.entry(key).or_insert(value);
        }
    }
    // Keep the eager-map fallback small and targeted to properties observed
    // missing on complete received creations. Shared projected nodes use the
    // same catalog lazily in get_property_from_world.
    for (key, wire_key) in [
        ("Archivable", "archivable"),
        ("Transparency", "Transparency"),
        ("HoldDuration", "HoldDuration"),
        ("Active", "Active"),
        ("Value", "Value"),
    ] {
        if properties.contains_key(key) || properties.contains_key(wire_key) {
            continue;
        }
        if let Ok((value, _)) = constructor_default(class, key) {
            properties.insert(wire_key.into(), value);
        }
    }
}

/// Constructor values from the pinned Player constructor trace in
/// runtime/player_defaults.inc. Player is not in the creatable-class catalog,
/// so completed received Player records need these explicit, evidence-backed
/// defaults while partial captures remain unknown.
fn player_constructor_defaults() -> [(&'static str, Json); 9] {
    [
        ("CameraMinZoomDistance", json!(0.5)),
        ("CameraMaxZoomDistance", json!(400.0)),
        ("DevEnableMouseLock", json!(true)),
        (
            "CameraMode",
            json!({"$enum":{"type":"CameraMode","name":"Classic","value":0}}),
        ),
        (
            "DevCameraOcclusionMode",
            json!({"$enum":{"type":"DevCameraOcclusionMode","name":"Zoom","value":0}}),
        ),
        (
            "DevComputerCameraMode",
            json!({"$enum":{"type":"DevComputerCameraMovementMode","name":"UserChoice","value":0}}),
        ),
        (
            "DevTouchCameraMode",
            json!({"$enum":{"type":"DevTouchCameraMovementMode","name":"UserChoice","value":0}}),
        ),
        (
            "DevComputerMovementMode",
            json!({"$enum":{"type":"DevComputerMovementMode","name":"UserChoice","value":0}}),
        ),
        (
            "DevTouchMovementMode",
            json!({"$enum":{"type":"DevTouchMovementMode","name":"UserChoice","value":0}}),
        ),
    ]
}

fn player_constructor_default(class: &str, key: &str) -> Option<Json> {
    (class == "Player")
        .then(|| {
            player_constructor_defaults()
                .into_iter()
                .find_map(|(name, value)| (name == key).then_some(value))
        })
        .flatten()
}

pub fn get_property_revision(ctx: &WorldRef, id: &str, key: &str) -> Option<u64> {
    let world = ctx.borrow();
    let node = world.nodes.get(id)?;
    if let Some(revision) = super::movement::received_joint_revision(node, key) {
        return Some(revision);
    }
    if let Some(revision) = node.revisions.get(key).and_then(Json::as_u64) {
        return Some(revision);
    }
    let wire_key = super::world_aliases::wire_key(&node.class_name, key);
    node.projected
        .as_ref()
        .and_then(|projected| {
            projected
                .revision(key)
                .ok()
                .flatten()
                .and_then(|value| value.as_u64())
                .or_else(|| {
                    wire_key.and_then(|wire| {
                        projected
                            .revision(wire)
                            .ok()
                            .flatten()
                            .and_then(|value| value.as_u64())
                    })
                })
        })
        .or_else(|| wire_key.and_then(|wire| node.revisions.get(wire).and_then(Json::as_u64)))
}

fn property_origin(ctx: &WorldRef, id: &str, key: &str) -> mlua::Result<String> {
    if let Some(origin) = super::joint_graph::origin(ctx, id, key) {
        return Ok(origin);
    }
    if let Some(origin) = super::movement::attachment_origin(ctx, id, key) {
        return Ok(origin);
    }
    raw_property_origin(ctx, id, key)
}

pub(crate) fn raw_property_origin(ctx: &WorldRef, id: &str, key: &str) -> mlua::Result<String> {
    let (class, local, unresolved_parent, projected) = {
        let world = ctx.borrow();
        let node = world
            .nodes
            .get(id)
            .ok_or_else(|| mlua::Error::RuntimeError("instance is not in this world".into()))?;
        if !node.local_overrides.contains_key(key)
            && super::movement::received_joint_property(node, key).is_some()
        {
            return Ok("derived_received".into());
        }
        (
            node.class_name.clone(),
            node.local_overrides.contains_key(key),
            node.parent
                .as_deref()
                .is_some_and(|parent| !world.nodes.contains_key(parent)),
            node.projected.clone(),
        )
    };
    if local {
        return Ok("local".into());
    }
    if key == "Name" || key == "ClassName" {
        return Ok("received".into());
    }
    if key == "Parent" {
        if unresolved_parent {
            return Ok("unresolved_reference".into());
        }
        return Ok("received".into());
    }
    if let Some(projected) = projected {
        if let Some(origin) = projected.origin(key).map_err(mlua::Error::RuntimeError)? {
            if let Some(origin) = origin.as_str() {
                return Ok(origin.to_owned());
            }
        }
        if let Some(wire_key) = super::world_aliases::wire_key(&class, key) {
            if let Some(origin) = projected
                .origin(wire_key)
                .map_err(mlua::Error::RuntimeError)?
            {
                if let Some(origin) = origin.as_str() {
                    return Ok("derived_received".into());
                }
            }
        }
    }
    let (has_property, is_local) = {
        let world = ctx.borrow();
        let node = &world.nodes[id];
        (node.properties.contains_key(key), !node.server_owned)
    };
    if is_local {
        if constructor_default(&class, key).is_ok() {
            return Ok("class_default".into());
        }
        if has_property {
            return Ok("local".into());
        }
    }
    if has_property {
        return Ok("received".into());
    }
    Ok("unavailable".into())
}

/// Store an engine feedback value without producing a script-authored mutation.
/// A supplied revision tracks the source receipt used by physics stale fences.
pub fn set_engine_property(
    ctx: &WorldRef,
    id: &str,
    key: &str,
    value: Json,
    revision: Option<u64>,
) -> mlua::Result<()> {
    let mut world = ctx.borrow_mut();
    let node = world.nodes.get_mut(id).ok_or_else(|| {
        mlua::Error::RuntimeError("engine update references an unknown instance".into())
    })?;
    if key == "Name" {
        node.name = value
            .as_str()
            .ok_or_else(|| mlua::Error::RuntimeError("Name must be a string".into()))?
            .into();
    }
    if key == "Parent" {
        node.parent = ref_id(&value);
    }
    node.local_overrides.insert(key.into(), value);
    if let Some(revision) = revision {
        node.revisions.insert(key.into(), json!(revision));
    }
    Ok(())
}

pub fn emit_property_changed(lua: &Lua, ctx: &WorldRef, id: &str, key: &str) -> mlua::Result<()> {
    emit_property_changed_direct(lua, ctx, id, key)?;
    refresh_derived_properties(lua)
}
pub(crate) fn emit_property_changed_direct(
    lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    key: &str,
) -> mlua::Result<()> {
    emit_property_changed_value(lua, ctx, id, key, None)
}

pub(crate) fn emit_property_changed_value(
    lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    key: &str,
    value: Option<&Json>,
) -> mlua::Result<()> {
    super::movement::joint_pose_changed(lua, ctx, id, key)?;
    if super::movement::attachment_transform_property(ctx, id, key)
        || super::joint_graph::derived_property(ctx, id, key)
    {
        return Ok(());
    }
    if is_a(ctx, id, "ValueBase") {
        if key == "Value" && super::signals::has_receivers(lua, ctx, id, "Changed")? {
            let current;
            let value = match value {
                Some(value) => value,
                None => {
                    current = get_property(ctx, id, key)?;
                    &current
                }
            };
            let mut args = MultiValue::new();
            args.push_back(value_from_json(lua, ctx, value)?);
            super::signals::fire(lua, ctx, id, "Changed", args)?;
        }
    } else {
        let mut args = MultiValue::new();
        args.push_back(Value::String(lua.create_string(key)?));
        super::signals::fire(lua, ctx, id, "Changed", args)?;
    }
    super::signals::fire(lua, ctx, id, &format!("property:{key}"), MultiValue::new())
}

fn hierarchy_changed(
    lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    old: Option<String>,
    new: Option<String>,
) -> mlua::Result<()> {
    super::player_lifecycle::local_parent(lua, ctx, id, old.clone(), new.clone())?;
    super::collection_events::parent_changed(lua, ctx, id, old.clone(), new.clone())?;
    super::hierarchy_events::local_parent(lua, ctx, id, old, new)
}

fn cleanup_destroyed_bindings(lua: &Lua, ctx: &WorldRef, id: &str) {
    super::player_lifecycle::forget(lua, ctx, id);
    let _ = super::bindables::cleanup_instance(lua, ctx, id);
    if ctx.borrow().nodes.class_name(id) == Some("RemoteFunction") {
        let _ = super::remotes::cleanup_instance(lua, ctx, id);
    }
    let _ = super::signals::cleanup_instance(lua, ctx, id);
}

pub fn set_property(
    lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    key: &str,
    value: Json,
) -> mlua::Result<()> {
    if key == "BrickColor" && is_a(ctx, id, "BasePart") {
        return set_property(
            lua,
            ctx,
            id,
            "Color",
            super::sequences::brick_color_json(&value)?,
        );
    }
    let part_color = key == "Color" && is_a(ctx, id, "BasePart");
    if part_color {
        super::sequences::color_json(&value)?;
    }
    let watch_brick = part_color
        && (super::signals::has_receivers(lua, ctx, id, "Changed")?
            || super::signals::has_receivers(lua, ctx, id, "property:BrickColor")?);
    let old_brick = if watch_brick {
        get_property(ctx, id, "BrickColor").ok()
    } else {
        None
    };
    let character_property = super::player_lifecycle::is_character(ctx, id, key);
    if character_property && !value.is_null() {
        let valid = value
            .get("$ref")
            .and_then(Json::as_str)
            .is_some_and(|target| {
                is_a(ctx, target, "Model")
                    && ctx
                        .borrow()
                        .nodes
                        .get(target)
                        .is_some_and(|node| !node.destroyed)
            });
        if !valid {
            return Err(mlua::Error::RuntimeError(
                "Character must be a Model in this world or nil".into(),
            ));
        }
    }
    let material_before = {
        let w = ctx.borrow();
        super::material_history::affects_property(&w, id, key)
            .then(|| super::material_history::capture(&w, None))
    };
    let old_value = get_property(ctx, id, key).ok();
    let mut world = ctx.borrow_mut();
    let base_local_revision = world
        .nodes
        .get(id)
        .ok_or_else(|| mlua::Error::RuntimeError("instance is not in this world".into()))?
        .local_revision;
    let old_parent = world
        .nodes
        .get(id)
        .ok_or_else(|| mlua::Error::RuntimeError("instance is not in this world".into()))?
        .parent
        .clone();
    let new_parent = if key == "Parent" {
        ref_id(&value)
    } else {
        old_parent.clone()
    };
    if key == "Parent" {
        if let Some(parent) = &new_parent {
            if !world.nodes.contains_key(parent) {
                return Err(mlua::Error::RuntimeError(
                    "Parent belongs to a different world snapshot".into(),
                ));
            }
            let mut cursor = Some(parent.clone());
            let mut seen = std::collections::HashSet::new();
            while let Some(current) = cursor {
                if current == id {
                    return Err(mlua::Error::RuntimeError(
                        "attempt to set Parent would create a cycle".into(),
                    ));
                }
                if !seen.insert(current.clone()) {
                    break;
                }
                cursor = world.nodes.get(&current).and_then(|n| n.parent.clone());
            }
        }
    }
    let node = world.nodes.get_mut(id).unwrap();
    // The engine keeps accepting property writes on a destroyed instance (it
    // just stays detached); PlayerModule's transparency controller relies on
    // that while its character is being respawned.
    if key == "ClassName" {
        return Err(mlua::Error::RuntimeError("ClassName is read-only".into()));
    }
    if node.class_name == "DataModel" && key == "JobId" {
        return Err(mlua::Error::RuntimeError("JobId is read-only".into()));
    }
    if key == "Name" {
        let name = value
            .as_str()
            .ok_or_else(|| mlua::Error::RuntimeError("Name must be a string".into()))?;
        node.name = name.into();
    } else if key == "Parent" {
        node.parent = ref_id(&value);
    }
    node.local_overrides.insert(key.into(), value.clone());
    world.local_revision = world.local_revision.wrapping_add(1).max(1);
    if matches!(key, "Parent" | "Disabled" | "Enabled" | "RunContext" | "Source") {
        world.actor_runnable_revision = world.actor_runnable_revision.wrapping_add(1);
    }
    let revision = world.local_revision;
    if let Some(node) = world.nodes.get_mut(id) {
        node.local_revision = revision;
    }
    let mut mutation = json!({"kind":"set","id":id,"property":key,"value":value});
    if world.local_namespace.starts_with("local:actor:") {
        mutation["_base_local_revision"] = json!(base_local_revision);
        mutation["_result_local_revision"] = json!(revision);
    }
    world
        .effects
        .entry("mutations")
        .or_insert_with(|| Json::Array(vec![]))
        .as_array_mut()
        .unwrap()
        .push(mutation);
    drop(world);
    if let Some(before) = material_before {
        let mut w = ctx.borrow_mut();
        let after = super::material_history::capture(&w, None);
        let sequence = w.sequence;
        w.material_history.record(before, after, false, sequence);
    }
    let changed = !old_value
        .as_ref()
        .is_some_and(|old| attribute_value_eq(old, &value));
    if key == "CurrentCamera" {
        super::camera::refresh(lua, ctx)?;
    }
    if character_property {
        super::player_lifecycle::local_character(lua, ctx, id, old_value.as_ref(), &value)?;
    } else if changed && key != "Parent" {
        emit_property_changed(lua, ctx, id, key)?;
    }
    if watch_brick {
        let new_brick = super::sequences::closest_brick_json(&value)?;
        if !old_brick
            .as_ref()
            .is_some_and(|old| attribute_value_eq(old, &new_brick))
        {
            emit_property_changed_direct(lua, ctx, id, "BrickColor")?;
        }
    }
    if key == "Parent" && old_parent != new_parent {
        hierarchy_changed(lua, ctx, id, old_parent, new_parent)?;
        refresh_derived_properties(lua)?;
    }
    if changed && matches!(key, "Name" | "Parent") {
        resolve_instance_handle_waiters(lua, ctx)?;
    }
    Ok(())
}

pub(crate) fn commit_curve_state(
    ctx: &WorldRef,
    id: &str,
    state: super::curves::CurveState,
) -> mlua::Result<()> {
    let mut world = ctx.borrow_mut();
    if !world.nodes.get(id).is_some_and(|node| !node.destroyed) {
        return Err(mlua::Error::RuntimeError(
            "curve instance is unavailable or destroyed".into(),
        ));
    }
    world.local_revision = world.local_revision.wrapping_add(1).max(1);
    let revision = world.local_revision;
    world.nodes.get_mut(id).unwrap().local_revision = revision;
    world.curve_state = state;
    Ok(())
}

pub fn is_a(ctx: &WorldRef, id: &str, class: &str) -> bool {
    let world = ctx.borrow();
    let Some(node_class) = world.nodes.class_name(id) else {
        return false;
    };
    if node_class == class || class == "Instance" || class == "Object" {
        return true;
    }
    class_chain(node_class)
        .iter()
        .any(|ancestor| ancestor == class)
}
pub fn children(ctx: &WorldRef, id: &str) -> Vec<String> {
    let world = ctx.borrow();
    world.nodes.children(id, &world.node_order)
}

pub fn descendants(ctx: &WorldRef, id: &str) -> Vec<String> {
    let world = ctx.borrow();
    // Temporary headers only: one world pass, then breadth-first traversal.
    // Repeated child scans make large downloaded animation trees quadratic.
    let mut by_parent = HashMap::<String, Vec<&str>>::new();
    for key in &world.node_order {
        let key: &str = key.as_ref();
        if let Some(parent) = world.nodes.live_parent(key) {
            by_parent.entry(parent).or_default().push(key);
        }
    }
    let mut pending = by_parent.remove(id).unwrap_or_default();
    let mut result = Vec::new();
    let mut seen = std::collections::HashSet::from([id]);
    let mut cursor = 0;
    while cursor < pending.len() {
        let child = pending[cursor];
        cursor += 1;
        if !seen.insert(child) {
            continue;
        }
        result.push(child.to_owned());
        if result.len() > 400_000 {
            break;
        }
        if let Some(children) = by_parent.remove(child) {
            pending.extend(children);
        }
    }
    result
}

pub fn ordered_ids(ctx: &WorldRef) -> Vec<String> {
    ctx.borrow()
        .node_order
        .iter()
        .map(|id| id.to_string())
        .collect()
}

pub(crate) fn ordered_matching_ids(ctx: &WorldRef, matches: impl Fn(&str) -> bool) -> Vec<String> {
    let world = ctx.borrow();
    let selected = world
        .nodes
        .matching_class(|class| matches(class))
        .map(|n| n.id.clone())
        .collect::<std::collections::HashSet<_>>();
    world
        .node_order
        .iter()
        .filter(|id| selected.contains(id.as_ref()))
        .map(|id| id.to_string())
        .collect()
}

pub(crate) fn watch_property_signal(
    lua: &Lua,
    ctx: &WorldRef,
    id: &str,
    key: &str,
) -> mlua::Result<()> {
    super::movement::watch_attachment_signal(lua, ctx, id, key)?;
    super::joint_graph::watch_signal(lua, ctx, id, key)?;
    super::geometry::mass_events::watch_signal(lua, ctx, id, key)
}

pub(crate) fn refresh_derived_properties(lua: &Lua) -> mlua::Result<()> {
    super::movement::refresh_attachments(lua)?;
    super::joint_graph::refresh(lua)?;
    super::geometry::mass_events::refresh(lua)
}

/// Actor-fork mutation journals carry private local revision fences until the
/// host merges them into the root world.
pub fn uses_actor_namespace(ctx: &WorldRef) -> bool {
    ctx.borrow().local_namespace.starts_with("local:actor:")
}

/// Advance a local instance revision and return the revision this write was
/// based on. Only actor-fork mutation journals expose that base privately.
pub fn bump_local_revision(ctx: &WorldRef, id: &str) -> mlua::Result<u64> {
    let mut world = ctx.borrow_mut();
    let base = world
        .nodes
        .get(id)
        .ok_or_else(|| mlua::Error::RuntimeError("instance is not in this world".into()))?
        .local_revision;
    world.local_revision = world.local_revision.wrapping_add(1).max(1);
    let revision = world.local_revision;
    if let Some(node) = world.nodes.get_mut(id) {
        node.local_revision = revision;
    }
    Ok(base)
}

fn class_parents() -> &'static HashMap<String, Vec<String>> {
    static PARENTS: OnceLock<HashMap<String, Vec<String>>> = OnceLock::new();
    PARENTS.get_or_init(|| {
        let bytes = include_str!("../../../runtime/class-parents-745.json");
        let mut parents = serde_json::from_str::<Json>(bytes)
            .ok()
            .and_then(|v| v.get("parents").cloned())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(class, value)| {
                let parents = match value {
                    Json::String(parent) => vec![parent],
                    Json::Array(values) => values
                        .into_iter()
                        .filter_map(|v| v.as_str().map(ToOwned::to_owned))
                        .collect(),
                    _ => return None,
                };
                Some((class, parents))
            })
            .collect::<HashMap<_, _>>();
        parents
            .entry("GlobalSettings".into())
            .or_insert_with(|| vec!["GenericSettings".into()]);
        parents
            .entry("GenericSettings".into())
            .or_insert_with(|| vec!["ServiceProvider".into()]);
        parents
            .entry("ServiceProvider".into())
            .or_insert_with(|| vec!["Instance".into()]);
        parents
            .entry("PluginManager".into())
            .or_insert_with(|| vec!["Instance".into()]);
        parents
    })
}
pub fn emit(ctx: &WorldRef, key: &str, mut value: Json) {
    let mut world = ctx.borrow_mut();
    if key == "mutations" && value.get("_base_local_revision").is_some() {
        if let Some(revision) = value
            .get("id")
            .and_then(Json::as_str)
            .and_then(|id| world.nodes.get(id))
            .map(|node| node.local_revision)
        {
            value["_result_local_revision"] = json!(revision);
        }
    }
    let values = world
        .effects
        .entry(key)
        .or_insert_with(|| Json::Array(vec![]));
    if let Json::Array(items) = values {
        items.push(value);
    }
}

#[path = "constructor_catalog.rs"]
mod constructor_catalog;
fn constructor_class(class: &str) -> Option<&'static Json> {
    constructor_catalog::class(class)
}

fn local_player_constructor() -> &'static Json {
    static PLAYER: OnceLock<Json> = OnceLock::new();
    PLAYER.get_or_init(|| {
        json!({
            "name":"Player","creatable":true,"properties":{
                "Archivable":{"value":true},"UserId":{"value":0},
                "Character":{"value":null},"GameplayPaused":{"value":false}
            }
        })
    })
}

fn local_part_enum_type(class: &str, key: &str) -> Option<&'static str> {
    let ancestry = class_chain(class);
    match key {
        "Material" if ancestry.iter().any(|parent| parent == "BasePart") => Some("Material"),
        "Shape" if ancestry.iter().any(|parent| parent == "Part") => Some("PartType"),
        // The pinned FormFactorPart constructor exposes this legacy enum
        // property on Part, WedgePart, and subclasses through ancestry.
        "formFactorRaw" if ancestry.iter().any(|parent| parent == "FormFactorPart") => {
            Some("FormFactor")
        }
        _ => None,
    }
}

fn local_part_mass_default(class: &str, key: &str) -> Option<Json> {
    let (enum_type, item, value) = match local_part_enum_type(class, key)? {
        "Material" => ("Material", "Plastic", 256),
        "PartType" => ("PartType", "Block", 1),
        "FormFactor" => ("FormFactor", "Brick", 1),
        _ => return None,
    };
    Some(json!({"$enum":{"type":enum_type,"name":item,"value":value}}))
}

pub(crate) fn constructor_default(class: &str, key: &str) -> mlua::Result<(Json, bool)> {
    if key == "BrickColor" && class_chain(class).iter().any(|base| base == "BasePart") {
        let (color, writable) = constructor_default(class, "Color")?;
        return Ok((super::sequences::closest_brick_json(&color)?, writable));
    }
    let description = constructor_class(class);
    if let Some(value) = super::joint_graph::local_mode_default(class, key) {
        return Ok((value, true));
    }
    if key == "FieldOfViewMode" && class_chain(class).iter().any(|c| c == "Camera") {
        return Ok((
            json!({"$enum":{"type":"FieldOfViewMode","name":"Vertical","value":0}}),
            true,
        ));
    }
    if key == "ClassName" {
        return Ok((json!(class), false));
    }
    if key == "Name" {
        let name = description
            .and_then(|desc| desc.get("name"))
            .and_then(Json::as_str)
            .unwrap_or(class);
        return Ok((json!(name), true));
    }
    if key == "Parent" {
        let creatable = description
            .and_then(|desc| desc.get("creatable"))
            .and_then(Json::as_bool)
            .unwrap_or(false);
        if creatable {
            return Ok((Json::Null, true));
        }
        return Err(mlua::Error::RuntimeError(
            "code-instantiated default is unavailable for this property".into(),
        ));
    }
    if let Some(default) = local_part_mass_default(class, key) {
        return Ok((default, true));
    }
    let lower_key = key.to_ascii_lowercase();
    let lookup_key = if key == "Archivable" {
        "archivable"
    } else {
        key
    };
    for candidate in class_chain(class) {
        let desc = constructor_class(&candidate);
        if let Some(property) = desc
            .and_then(|desc| desc.get("properties"))
            .and_then(|props| props.get(lookup_key).or_else(|| props.get(&lower_key)))
        {
            let Some(value) = property.get("value") else {
                break;
            };
            return Ok((
                value.clone(),
                property
                    .get("writable")
                    .and_then(Json::as_bool)
                    .unwrap_or(false),
            ));
        }
    }
    Err(mlua::Error::RuntimeError(
        "code-instantiated default is unavailable for this property".into(),
    ))
}

fn json_property_equal(left: &Json, right: &Json) -> bool {
    match (left.as_f64(), right.as_f64()) {
        (Some(a), Some(b)) => a == b,
        _ => left == right,
    }
}

/// Create one metadata-backed client-local object. This is shared by Instance.new
/// and source playback so default properties and mutation output stay identical.
pub fn create_instance(
    lua: &Lua,
    ctx: &WorldRef,
    class: &str,
    parent: Option<&str>,
) -> mlua::Result<Value> {
    create_initialized_instance(lua, ctx, class, parent, None)
}

/// Animation downloads arrive as complete initial state. One create row keeps
/// each node atomic when an Actor replays it into the shared world.
pub(crate) fn create_animation_node(
    lua: &Lua,
    ctx: &WorldRef,
    class: &str,
    name: &str,
    parent: Option<&str>,
    properties: JsonMap<String, Json>,
) -> mlua::Result<Value> {
    if !matches!(
        class,
        "KeyframeSequence"
            | "Keyframe"
            | "Pose"
            | "NumberPose"
            | "KeyframeMarker"
            | "AnimationRigData"
            | "Folder"
            | "FloatCurve"
    ) || name.len() > 1024
        || serde_json::to_vec(&properties)
            .map_err(mlua::Error::external)?
            .len()
            > 64 * 1024
    {
        return Err(mlua::Error::RuntimeError(
            "animation node exceeds supported limits".into(),
        ));
    }
    create_initialized_instance(lua, ctx, class, parent, Some((name, properties)))
}

fn create_initialized_instance(
    lua: &Lua,
    ctx: &WorldRef,
    class: &str,
    parent: Option<&str>,
    initial: Option<(&str, JsonMap<String, Json>)>,
) -> mlua::Result<Value> {
    if class == "Player" && !super::scheduler::has_capability(lua, "WritePlayer") {
        return Err(mlua::Error::RuntimeError(
            "Player construction: lacking capability WritePlayer".into(),
        ));
    }
    let description = (if class == "Player" {
        Some(local_player_constructor())
    } else {
        constructor_class(class)
    })
    .filter(|v| v.get("creatable").and_then(Json::as_bool) == Some(true))
    .ok_or_else(|| {
        mlua::Error::RuntimeError(format!(
            "class '{class}' is not a supported creatable class"
        ))
    })?;
    let parent = parent.map(ToOwned::to_owned);
    if let Some(id) = &parent {
        if !ctx
            .borrow()
            .nodes
            .get(id)
            .is_some_and(|node| !node.destroyed)
        {
            return Err(mlua::Error::RuntimeError(
                "Parent belongs to a different world snapshot".into(),
            ));
        }
    }
    let (id, name, properties) = {
        let mut world = ctx.borrow_mut();
        if world.nodes.len() >= 501_000 {
            return Err(mlua::Error::RuntimeError(
                "local instance limit exceeded".into(),
            ));
        }
        let id = allocate_local_instance_id(&mut world)?;
        let mut name = description
            .get("name")
            .and_then(Json::as_str)
            .unwrap_or(class)
            .to_owned();
        let mut properties = JsonMap::new();
        if let Some(defaults) = description.get("properties").and_then(Json::as_object) {
            for (key, record) in defaults {
                if let Some(value) = record.get("value") {
                    properties.insert(key.clone(), value.clone());
                }
            }
        }
        if let Some((initial_name, initial_properties)) = &initial {
            name = (*initial_name).to_owned();
            properties.extend(initial_properties.clone());
        }
        (id, name, properties)
    };
    let initial_parent = initial.as_ref().and(parent.clone());
    let node = Node {
        id: id.clone(),
        class_name: class.into(),
        name: name.clone(),
        parent: initial_parent.clone(),
        destroyed: false,
        server_owned: false,
        received_creation_complete: false,
        attributes_known: true,
        collection_tags_known: true,
        projected: None,
        data: crate::rust_host::world::NodeData {
            properties: properties.clone(),
            local_overrides: JsonMap::new(),
            attributes: JsonMap::new(),
            revisions: JsonMap::new(),
            local_revision: 0,
            collection_tags: Vec::new(),
        }
        .pack(),
    };
    {
        let mut world = ctx.borrow_mut();
        world.nodes.insert(id.clone(), node);
        world.record_order(&id);
    }
    emit(
        ctx,
        "mutations",
        json!({"kind":"create","node":{"id":id,"class_name":class,"name":name,"parent":initial_parent,
        "properties":properties,"attributes":{},"attributes_known":true,"collection_tags":[],"collection_tags_known":true,
        "destroyed":false,"server_owned":false,"property_revisions":{}}}),
    );
    if initial.is_some() {
        hierarchy_changed(lua, ctx, &id, None, parent.clone())?;
    } else if let Some(parent) = parent {
        set_property(lua, ctx, &id, "Parent", json!({"$ref":parent}))?;
    }
    instance(lua, ctx, &id)
}

/// Create a small host-managed local service result without admitting new
/// instance classes to the script-facing `Instance.new` constructor.
pub fn create_internal_node(
    lua: &Lua,
    ctx: &WorldRef,
    class: &str,
    parent: Option<&str>,
    properties: serde_json::Map<String, Json>,
) -> mlua::Result<Value> {
    if !matches!(
        class,
        "PlayerMouse"
            | "TextChatMessage"
            | "StatsItem"
            | "PlayerScripts"
            | "UserSettings"
            | "UserGameSettings"
            | "GlobalSettings"
            | "PluginManager"
    ) {
        return Err(mlua::Error::RuntimeError(
            "class is not available to internal node creation".into(),
        ));
    }
    if serde_json::to_vec(&properties)
        .map_err(mlua::Error::external)?
        .len()
        > 64 * 1024
    {
        return Err(mlua::Error::RuntimeError(
            "internal node properties exceed the 64 KiB limit".into(),
        ));
    }
    let (id, node) = {
        let mut world = ctx.borrow_mut();
        if let Some(parent) = parent {
            if !world.nodes.get(parent).is_some_and(|node| !node.destroyed) {
                return Err(mlua::Error::RuntimeError(
                    "internal node parent is unavailable".into(),
                ));
            }
        }
        let id = allocate_local_instance_id(&mut world)?;
        let node = Node {
            id: id.clone(),
            class_name: class.to_owned(),
            name: class.to_owned(),
            parent: parent.map(ToOwned::to_owned),
            destroyed: false,
            server_owned: false,
            received_creation_complete: false,
            attributes_known: true,
            collection_tags_known: true,
            projected: None,
            data: crate::rust_host::world::NodeData {
                properties,
                local_overrides: JsonMap::new(),
                attributes: JsonMap::new(),
                revisions: JsonMap::new(),
                local_revision: 0,
                collection_tags: Vec::new(),
            }
            .pack(),
        };
        world.record_order(&id);
        world.nodes.insert(id.clone(), node.clone());
        (id, node)
    };
    emit(
        ctx,
        "mutations",
        json!({"kind":"create","node":{
            "id":node.id,"class_name":node.class_name,"name":node.name,
            "parent":node.parent.as_ref().map(|id| json!(id)).unwrap_or(Json::Null),
            "properties":node.properties,"attributes":node.attributes,
            "attributes_known":node.attributes_known,"collection_tags":node.collection_tags,
            "collection_tags_known":node.collection_tags_known,"destroyed":false,"server_owned":false,
            "property_revisions":node.revisions
        }}),
    );
    if let Some(parent) = parent {
        hierarchy_changed(lua, ctx, &id, None, Some(parent.to_owned()))?;
    }
    instance(lua, ctx, &id)
}

/// Recreate the two local engine singletons without adding a server service or
/// producing a script-visible create mutation.
pub fn ensure_engine_global_node(lua: &Lua, ctx: &WorldRef, class: &str) -> mlua::Result<Value> {
    let id = match class {
        "GlobalSettings" => "local:engine:GlobalSettings",
        "PluginManager" => "local:engine:PluginManager",
        _ => {
            return Err(mlua::Error::RuntimeError(
                "class is not an engine global singleton".into(),
            ));
        }
    };
    {
        let mut world = ctx.borrow_mut();
        if world.nodes.get(id).is_some_and(|node| !node.destroyed) {
            // Keep the existing singleton stable.
        } else {
            if world.nodes.len() >= 501_000 {
                return Err(mlua::Error::RuntimeError(
                    "engine global node limit exceeded".into(),
                ));
            }
            if !world.nodes.contains_key(id) {
                world.record_order(id);
            }
            world.nodes.insert(
                id.to_owned(),
                Node {
                    id: id.to_owned(),
                    class_name: class.to_owned(),
                    name: class.to_owned(),
                    parent: None,
                    destroyed: false,
                    server_owned: false,
                    received_creation_complete: false,
                    attributes_known: true,
                    collection_tags_known: true,
                    projected: None,
                    data: crate::rust_host::world::NodeData {
                        properties: JsonMap::new(),
                        local_overrides: JsonMap::new(),
                        attributes: JsonMap::new(),
                        revisions: JsonMap::new(),
                        local_revision: 0,
                        collection_tags: Vec::new(),
                    }
                    .pack(),
                },
            );
        }
    };
    instance(lua, ctx, id)
}

fn local_service_supported(class: &str) -> bool {
    matches!(
        class,
        "VirtualInputManager"
            | "VirtualUser"
            | "ContentProvider"
            | "Teams"
            | "ReplicatedFirst"
            | "VRService"
            | "SoundService"
            | "PhysicsService"
            | "MaterialService"
            | "JointsService"
            | "EncodingService"
            | "PathfindingService"
            | "CorePackages"
            | "SharedTableRegistry"
            | "TextService"
            | "RunService"
            | "HttpService"
            | "ServerStorage"
            | "ServerScriptService"
            | "GuiService"
            | "UserInputService"
            | "ContextActionService"
            | "Lighting"
            | "CoreGui"
            | "Debris"
            | "CollectionService"
            | "TweenService"
            | "AnimationClipProvider"
            | "KeyframeSequenceProvider"
            | "TestService"
            | "Stats"
    )
}

/// Return the stable client-owned local service Instance, creating it only for
/// the pinned list of services that the legacy runtime supplies locally.
pub fn ensure_client_service(lua: &Lua, ctx: &WorldRef, class: &str) -> mlua::Result<Value> {
    if !local_service_supported(class) {
        return Err(mlua::Error::RuntimeError(format!(
            "local service {class} is unsupported"
        )));
    }
    let existing = {
        let mut world = ctx.borrow_mut();
        let existing_id = world
            .nodes
            .of_class(class)
            .find(|node| !node.server_owned && !node.destroyed)
            .map(|node| node.id.clone());
        if let Some(id) = existing_id {
            world.services.insert(class.to_owned(), id.clone());
            Some(id)
        } else {
            None
        }
    };
    if let Some(id) = existing {
        return instance(lua, ctx, &id);
    }
    let (id, game_id) = {
        let mut world = ctx.borrow_mut();
        if world.nodes.len() >= 400_000 {
            return Err(mlua::Error::RuntimeError(
                "world node limit exceeded".into(),
            ));
        }
        let game_id = world
            .nodes
            .first_live_of_class("DataModel")
            .map(|node| node.id.clone())
            .ok_or_else(|| {
                mlua::Error::RuntimeError(
                    "DataModel is unavailable for local service creation".into(),
                )
            })?;
        let id = allocate_local_instance_id(&mut world)?;
        let node = Node {
            id: id.clone(),
            class_name: class.to_owned(),
            name: class.to_owned(),
            parent: Some(game_id.clone()),
            destroyed: false,
            server_owned: false,
            received_creation_complete: false,
            attributes_known: true,
            collection_tags_known: true,
            projected: None,
            data: crate::rust_host::world::NodeData {
                properties: JsonMap::from_iter([("Archivable".into(), json!(true))]),
                local_overrides: JsonMap::new(),
                attributes: JsonMap::new(),
                revisions: JsonMap::new(),
                local_revision: 0,
                collection_tags: Vec::new(),
            }
            .pack(),
        };
        world.record_order(&id);
        world.nodes.insert(id.clone(), node);
        world.services.insert(class.to_owned(), id.clone());
        (id, game_id)
    };
    emit(
        ctx,
        "mutations",
        json!({"kind":"create","node":{
            "id":id,"class_name":class,"name":class,"parent":game_id,
            "properties":{"Archivable":true},"attributes":{},"attributes_known":true,
            "collection_tags":[],"collection_tags_known":true,"destroyed":false,
            "server_owned":false,"property_revisions":{}
        }}),
    );
    hierarchy_changed(lua, ctx, &id, None, Some(game_id))?;
    instance(lua, ctx, &id)
}

/// Adopt one locally-created node from an Actor VM into the shared root world.
/// The ID must use this Actor's reserved namespace, and the serialized create
/// row must match the node the Actor VM actually created.
pub fn apply_actor_create(
    lua: &Lua,
    root: &WorldRef,
    actor_world: &WorldRef,
    actor_id: &str,
    record: &Json,
) -> mlua::Result<()> {
    apply_actor_create_for_replay(lua, root, actor_world, actor_id, record, false)
}

pub(crate) fn apply_actor_create_for_replay(
    lua: &Lua,
    root: &WorldRef,
    actor_world: &WorldRef,
    actor_id: &str,
    record: &Json,
    retiring: bool,
) -> mlua::Result<()> {
    let id = record
        .get("id")
        .and_then(Json::as_str)
        .ok_or_else(|| mlua::Error::RuntimeError("Actor create row needs an ID".into()))?;
    let mut encoded_id = String::with_capacity(actor_id.len() * 2);
    for byte in actor_id.as_bytes() {
        use std::fmt::Write as _;
        let _ = write!(&mut encoded_id, "{byte:02x}");
    }
    let namespace = format!("local:actor:{encoded_id}:");
    if !id.starts_with(&namespace) {
        return Err(mlua::Error::RuntimeError(
            "Actor create ID is outside its local namespace".into(),
        ));
    }
    let serialized_size = serde_json::to_vec(record)
        .map_err(mlua::Error::external)?
        .len();
    if serialized_size > 1024 * 1024 {
        return Err(mlua::Error::RuntimeError(
            "Actor create record exceeds the 1 MiB limit".into(),
        ));
    }
    let (source, parent) = {
        let actor = actor_world.borrow();
        let source = actor.nodes.get(id).cloned().ok_or_else(|| {
            mlua::Error::RuntimeError("Actor create row references no local node".into())
        })?;
        if source.server_owned || source.projected.is_some() || (source.destroyed && !retiring) {
            return Err(mlua::Error::RuntimeError(
                "Actor create row cannot adopt a received or destroyed node".into(),
            ));
        }
        let data = record
            .as_object()
            .ok_or_else(|| mlua::Error::RuntimeError("invalid Actor create row".into()))?;
        let class = data
            .get("class_name")
            .and_then(Json::as_str)
            .ok_or_else(|| mlua::Error::RuntimeError("Actor create row needs class_name".into()))?;
        if class != source.class_name {
            return Err(mlua::Error::RuntimeError(
                "Actor create row class does not match its local node".into(),
            ));
        }
        let creatable = constructor_class(class)
            .and_then(|description| description.get("creatable"))
            .and_then(Json::as_bool)
            .unwrap_or(false);
        if !creatable && !local_service_supported(class) {
            return Err(mlua::Error::RuntimeError(
                "Actor create class is not a supported local constructor or service".into(),
            ));
        }
        let parent = data.get("parent").and_then(ref_id);
        (source, parent)
    };
    let data = record
        .as_object()
        .ok_or_else(|| mlua::Error::RuntimeError("invalid Actor create row".into()))?;
    let name = data
        .get("name")
        .and_then(Json::as_str)
        .ok_or_else(|| mlua::Error::RuntimeError("Actor create row needs name".into()))?;
    let properties = data
        .get("properties")
        .and_then(Json::as_object)
        .ok_or_else(|| mlua::Error::RuntimeError("Actor create row needs properties".into()))?;
    let attributes = data
        .get("attributes")
        .and_then(Json::as_object)
        .cloned()
        .unwrap_or_default();
    let attributes_known = data
        .get("attributes_known")
        .and_then(Json::as_bool)
        .unwrap_or(true);
    let collection_tags = data
        .get("collection_tags")
        .and_then(Json::as_array)
        .cloned()
        .unwrap_or_default();
    let collection_tags_known = data
        .get("collection_tags_known")
        .and_then(Json::as_bool)
        .unwrap_or(true);
    let collection_tags = validate_tags(&collection_tags, collection_tags_known)
        .map_err(mlua::Error::RuntimeError)?;
    let property_revisions = data
        .get("property_revisions")
        .and_then(Json::as_object)
        .cloned()
        .unwrap_or_default();
    let scale_state = super::movement::import_scale_baseline(root, id, record)?;
    let curve_state = super::curves::import_state(root, id, record)?;
    if data.get("server_owned").and_then(Json::as_bool) == Some(true)
        || data.get("destroyed").and_then(Json::as_bool) == Some(true)
    {
        return Err(mlua::Error::RuntimeError(
            "Actor create row cannot adopt a server-owned or destroyed node".into(),
        ));
    }
    {
        let mut world = root.borrow_mut();
        if world.nodes.contains_key(id) {
            return Err(mlua::Error::RuntimeError(
                "Actor create ID already exists in the root world".into(),
            ));
        }
        if let Some(parent) = &parent {
            if !world.nodes.get(parent).is_some_and(|node| !node.destroyed) {
                return Err(mlua::Error::RuntimeError(
                    "Actor create parent is unavailable in the root world".into(),
                ));
            }
        }
        if local_service_supported(&source.class_name) {
            let expected_parent = world
                .nodes
                .first_live_of_class("DataModel")
                .map(|node| node.id.as_str());
            if parent.as_deref() != expected_parent
                || name != source.class_name
                || !attributes.is_empty()
                || !attributes_known
                || !collection_tags.is_empty()
                || !collection_tags_known
                || properties.len() != 1
                || properties.get("Archivable") != Some(&json!(true))
            {
                return Err(mlua::Error::RuntimeError(
                    "Actor local service create record does not match its engine defaults".into(),
                ));
            }
            if world
                .services
                .get(&source.class_name)
                .is_some_and(|existing| existing != id)
            {
                return Err(mlua::Error::RuntimeError(
                    "Actor local service already exists in the root world".into(),
                ));
            }
        }
        let material_before = super::material_history::capture(&world, None);
        let mut node = source;
        node.name = name.to_owned();
        node.parent = parent.clone();
        node.properties = properties.clone();
        node.local_overrides.clear();
        node.attributes = attributes;
        node.attributes_known = attributes_known;
        node.collection_tags = collection_tags;
        node.collection_tags_known = collection_tags_known;
        node.revisions = property_revisions;
        node.local_revision = 0;
        node.server_owned = false;
        node.destroyed = false;
        node.projected = None;
        world.record_order(id);
        world.nodes.insert(id.to_owned(), node.clone());
        world.scale_state = scale_state;
        world.curve_state = curve_state;
        if local_service_supported(&node.class_name) {
            world
                .services
                .insert(node.class_name.clone(), id.to_owned());
        }
        let mut mutation = json!({"kind":"create","node":{
            "id":node.id,"class_name":node.class_name,"name":node.name,
            "parent":node.parent.as_ref().map(|id| json!(id)).unwrap_or(Json::Null),
            "properties":node.properties,"attributes":node.attributes,
            "attributes_known":node.attributes_known,"collection_tags":node.collection_tags,
            "collection_tags_known":node.collection_tags_known,
            "property_revisions":node.revisions,"destroyed":false,"server_owned":false
        }});
        mutation["actor_origin"] = json!(actor_id);
        let curve = world.curve_state.record(id);
        if !curve.is_null() {
            mutation["node"]["curve_state"] = curve;
        }
        world
            .effects
            .entry("mutations")
            .or_insert_with(|| Json::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| mlua::Error::RuntimeError("mutation effect list is invalid".into()))?
            .push(mutation);
        let after = super::material_history::capture(&world, None);
        let sequence = world.sequence;
        world
            .material_history
            .record(material_before, after, false, sequence);
    }
    hierarchy_changed(lua, root, id, None, parent)?;
    Ok(())
}

fn is_archivable(ctx: &WorldRef, id: &str) -> mlua::Result<bool> {
    match get_property(ctx, id, "Archivable") {
        Ok(value) => value.as_bool().ok_or_else(|| {
            mlua::Error::RuntimeError(
                "Archivable is unavailable; cannot faithfully clone this instance".into(),
            )
        }),
        Err(_) => {
            let class = ctx
                .borrow()
                .nodes
                .get(id)
                .map(|n| n.class_name.clone())
                .unwrap_or_default();
            constructor_class(&class)
                .and_then(|c| c.get("properties"))
                .and_then(|props| props.get("Archivable"))
                .and_then(|p| p.get("value"))
                .and_then(Json::as_bool)
                .ok_or_else(|| {
                    mlua::Error::RuntimeError(
                        "Archivable is unavailable; cannot faithfully clone this instance".into(),
                    )
                })
        }
    }
}

fn remap_clone_refs(
    value: &mut Json,
    id_map: &HashMap<String, String>,
    depth: usize,
) -> mlua::Result<()> {
    if depth > 128 {
        return Err(mlua::Error::RuntimeError(
            "clone reference nesting limit exceeded".into(),
        ));
    }
    if let Some(object) = value.as_object_mut() {
        if object.len() == 1 {
            for key in ["$ref", "$instance_handle"] {
                if let Some(id) = object
                    .get(key)
                    .and_then(Json::as_str)
                    .map(ToOwned::to_owned)
                {
                    if let Some(clone_id) = id_map.get(&id) {
                        object.insert(key.into(), json!(clone_id));
                    }
                    return Ok(());
                }
            }
        }
        for item in object.values_mut() {
            remap_clone_refs(item, id_map, depth + 1)?;
        }
    } else if let Some(array) = value.as_array_mut() {
        for item in array {
            remap_clone_refs(item, id_map, depth + 1)?;
        }
    }
    Ok(())
}

pub fn clone_instance(lua: &Lua, ctx: &WorldRef, root_id: &str) -> mlua::Result<Value> {
    if !ctx.borrow().nodes.contains_key(root_id) {
        return Err(mlua::Error::RuntimeError(
            "instance is not in this world".into(),
        ));
    }
    if !is_archivable(ctx, root_id)? {
        return Ok(Value::Nil);
    }
    let mut selected = Vec::new();
    let mut pending = vec![root_id.to_owned()];
    let mut seen = std::collections::HashSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        if !is_archivable(ctx, &id)? {
            continue;
        }
        let kids = children(ctx, &id);
        selected.push(id);
        pending.extend(kids.into_iter().rev());
        if selected.len() > 400_000 {
            return Err(mlua::Error::RuntimeError(
                "clone node limit exceeded".into(),
            ));
        }
    }
    let mut id_map = HashMap::with_capacity(selected.len());
    {
        let mut world = ctx.borrow_mut();
        if world.nodes.len().saturating_add(selected.len()) > 501_000 {
            return Err(mlua::Error::RuntimeError(
                "local instance limit exceeded".into(),
            ));
        }
        for source in &selected {
            let id = allocate_local_instance_id(&mut world)?;
            id_map.insert(source.clone(), id);
        }
    }
    let mut clones = Vec::with_capacity(selected.len());
    for source_id in &selected {
        let mut original = {
            let world = ctx.borrow();
            world
                .nodes
                .get(source_id)
                .cloned()
                .ok_or_else(|| mlua::Error::RuntimeError("clone source disappeared".into()))?
        };
        let mut properties = if let Some(projected) = &original.projected {
            match projected
                .section("properties")
                .map_err(mlua::Error::RuntimeError)?
            {
                ReadOnlyField::Value(Json::Object(map)) => map,
                _ => JsonMap::new(),
            }
        } else {
            original.properties.clone()
        };
        properties.extend(
            original
                .local_overrides
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        world_pose_batch::preserve_pivot_mode(&original, &mut properties);
        super::movement::preserve_scale(ctx, source_id, &mut properties);
        super::movement::preserve_attachment_pose(&original, &mut properties);
        super::movement::preserve_joint_pose(&original, &mut properties);
        properties.remove("Name");
        properties.remove("Parent");
        for value in properties.values_mut() {
            remap_clone_refs(value, &id_map, 0)?;
        }
        let attrs = serde_json::to_value(&original.attributes).map_err(mlua::Error::external)?;
        let mut attributes = attrs;
        remap_clone_refs(&mut attributes, &id_map, 0)?;
        let parent = if source_id == root_id {
            None
        } else {
            original
                .parent
                .as_ref()
                .and_then(|parent| id_map.get(parent))
                .cloned()
        };
        let clone_id = id_map[source_id].clone();
        let collection_tags = original
            .data
            .as_mut()
            .map(|data| std::mem::take(&mut data.collection_tags))
            .unwrap_or_default();
        let node = Node {
            id: clone_id,
            class_name: original.class_name,
            name: original.name,
            parent,
            destroyed: false,
            server_owned: false,
            received_creation_complete: false,
            attributes_known: original.attributes_known,
            collection_tags_known: original.collection_tags_known,
            projected: None,
            data: crate::rust_host::world::NodeData {
                properties,
                local_overrides: JsonMap::new(),
                attributes: attributes.as_object().cloned().unwrap_or_default(),
                revisions: JsonMap::new(),
                local_revision: 0,
                collection_tags,
            }
            .pack(),
        };
        clones.push(node);
    }
    let root_clone = id_map[root_id].clone();
    let scale_state = super::movement::clone_scale_state(ctx, &id_map)?;
    let curve_state = super::curves::clone_state(ctx, &id_map)?;
    world_model_scale::check_actor_clone_journal(ctx, &clones, &scale_state, &curve_state)?;
    {
        let mut world = ctx.borrow_mut();
        world.scale_state = scale_state;
        world.curve_state = curve_state;
        for node in &clones {
            world.record_order(&node.id);
            world.nodes.insert(node.id.clone(), node.clone());
        }
    }
    for (source_id, clone_id) in &id_map {
        let is_script = ctx.borrow().nodes.get(source_id).is_some_and(|node| {
            matches!(
                node.class_name.as_str(),
                "Script" | "LocalScript" | "ModuleScript"
            )
        });
        if is_script {
            super::lifecycle::record_clone(lua, source_id, clone_id)?;
        }
    }
    for node in clones {
        let record = world_model_scale::clone_record(
            &node,
            super::movement::export_scale_baseline(ctx, &node.id),
            ctx.borrow().curve_state.record(&node.id),
        );
        emit(ctx, "mutations", json!({"kind":"create","node":record}));
    }
    instance(lua, ctx, &root_clone)
}

pub fn destroy_instance(lua: &Lua, ctx: &WorldRef, root_id: &str) -> mlua::Result<()> {
    if ctx
        .borrow()
        .nodes
        .get(root_id)
        .is_none_or(|node| node.destroyed)
    {
        return Ok(());
    }
    let material_before = super::material_history::capture(&ctx.borrow(), None);
    let mut selected = Vec::new();
    let mut pending = vec![root_id.to_owned()];
    let mut seen = std::collections::HashSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        pending.extend(children(ctx, &id).into_iter().rev());
        selected.push(id);
        if selected.len() > 400_000 {
            return Err(mlua::Error::RuntimeError(
                "destroy subtree limit exceeded".into(),
            ));
        }
    }
    let actor_internal_revision = uses_actor_namespace(ctx);
    let base_revisions = if actor_internal_revision {
        let world = ctx.borrow();
        selected
            .iter()
            .filter_map(|id| {
                world
                    .nodes
                    .get(id)
                    .map(|node| (id.clone(), json!(node.local_revision)))
            })
            .collect::<JsonMap<String, Json>>()
    } else {
        JsonMap::new()
    };
    let mut hierarchy_events = super::hierarchy_events::prepare(
        lua,
        ctx,
        &[json!({"kind":"destroy","id":root_id})],
        None,
    )?;
    super::collection_events::destroying(lua, ctx, root_id)?;
    for id in &selected {
        let old_parent = ctx.borrow().nodes.parent(id).flatten();
        super::player_lifecycle::local_parent(lua, ctx, id, old_parent, None)?;
    }
    hierarchy_events.publish(lua, ctx, 0)?;
    super::lifecycle::destroy_script_instances(lua, &selected)?;
    // Preserve the old ancestry until every descendant's removal event has
    // been queued; detaching the root first loses its ancestors for children.
    for id in &selected {
        cleanup_destroyed_bindings(lua, ctx, id);
        if let Some(node) = ctx.borrow_mut().nodes.get_mut(id) {
            node.destroyed = true;
            node.parent = None;
            node.local_overrides.insert("Parent".into(), Json::Null);
        }
    }
    {
        let mut world = ctx.borrow_mut();
        world.actor_runnable_revision = world.actor_runnable_revision.wrapping_add(1);
    }
    {
        let mut w = ctx.borrow_mut();
        let after = super::material_history::capture(&w, None);
        let sequence = w.sequence;
        w.material_history
            .record(material_before, after, false, sequence);
    }
    let mut mutation = json!({"kind":"destroy","ids":selected});
    for id in &selected {
        ctx.borrow_mut().scale_state.remove(id);
    }
    for id in &selected {
        ctx.borrow_mut().curve_state.remove(id);
    }
    if actor_internal_revision {
        mutation["_base_local_revisions"] = Json::Object(base_revisions);
    }
    emit(ctx, "mutations", mutation);
    resolve_instance_handle_waiters(lua, ctx)?;
    super::movement::joint_pose_changed(lua, ctx, root_id, "Parent")?;
    Ok(())
}

struct InstanceHandle {
    id: String,
    world: Weak<RefCell<World>>,
}
impl InstanceHandle {
    fn ctx(&self) -> mlua::Result<WorldRef> {
        self.world
            .upgrade()
            .ok_or_else(|| mlua::Error::RuntimeError("world has been released".into()))
    }
}
impl UserData for InstanceHandle {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field("__type", "Instance");
        fields.add_field_method_get("Name", |_, this| {
            Ok(this
                .ctx()?
                .borrow()
                .nodes
                .get(&this.id)
                .map(|n| n.name.clone())
                .unwrap_or_default())
        });
        fields.add_field_method_set("Name", |lua, this, name: String| {
            set_property(lua, &this.ctx()?, &this.id, "Name", json!(name))
        });
        fields.add_field_method_get("ClassName", |_, this| {
            Ok(this
                .ctx()?
                .borrow()
                .nodes
                .get(&this.id)
                .map(|n| n.class_name.clone())
                .unwrap_or_default())
        });
        fields.add_field_method_get("Parent", |lua, this| {
            let ctx = this.ctx()?;
            let parent = ctx
                .borrow()
                .nodes
                .get(&this.id)
                .and_then(|n| n.parent.clone());
            match parent {
                Some(id) => instance(lua, &ctx, &id),
                None => Ok(Value::Nil),
            }
        });
        fields.add_field_method_set("Parent", |lua, this, parent: Value| {
            let ctx = this.ctx()?;
            let parent_id = instance_id(&parent);
            if !parent.is_nil() && !instance_belongs_to(&parent, &ctx) {
                return Err(mlua::Error::RuntimeError(
                    "Parent belongs to a different world snapshot".into(),
                ));
            }
            if let Some(ref id) = parent_id {
                if !ctx.borrow().nodes.contains_key(id) {
                    return Err(mlua::Error::RuntimeError(
                        "Parent belongs to a different world snapshot".into(),
                    ));
                }
            }
            set_property(
                lua,
                &ctx,
                &this.id,
                "Parent",
                parent_id.map(|id| json!({"$ref":id})).unwrap_or(Json::Null),
            )
        });
    }
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Index, |lua, this, raw_key: String| {
            let ctx = this.ctx()?;
            let class = ctx
                .borrow()
                .nodes
                .get(&this.id)
                .map(|n| n.class_name.clone())
                .unwrap_or_default();
            let key = canonical_member(&class, &raw_key);
            if key == "DataCost" && !super::scheduler::has_capability(lua, "LocalUser") {
                return Err(mlua::Error::RuntimeError(
                    "DataCost: lacking capability LocalUser".into(),
                ));
            }
            if key == "WaitForChild" {
                return Ok(Value::Function(child_wait_function(lua)?));
            }
            if matches!(key.as_str(), "Name" | "ClassName" | "Parent") {
                return Ok(Value::Nil);
            }
            let method = {
                let world = ctx.borrow();
                class_chain(&class)
                    .iter()
                    .find_map(|class| world.methods.get(&(class.clone(), key.clone())).cloned())
            };
            if let Some(method) = method {
                let id = this.id.clone();
                let ctx2 = ctx.clone();
                return Ok(Value::Function(lua.create_function(
                    move |lua, mut args: MultiValue| {
                        let receiver = args.pop_front().ok_or_else(|| {
                            mlua::Error::RuntimeError("Instance method requires a receiver".into())
                        })?;
                        if instance_id(&receiver).as_deref() != Some(id.as_str())
                            || !instance_belongs_to(&receiver, &ctx2)
                        {
                            return Err(mlua::Error::RuntimeError(
                                "Instance method receiver belongs to a different world or instance"
                                    .into(),
                            ));
                        }
                        method(lua, &ctx2, &id, args)
                    },
                )?));
            }
            if class == "DataModel" && key == "Workspace" {
                if let Some(id) = ctx.borrow().services.get("Workspace").cloned() {
                    if ctx.borrow().nodes.contains_key(&id) {
                        return instance(lua, &ctx, &id);
                    }
                }
            }
            let getters = ctx.borrow().getters.get(&key).cloned().unwrap_or_default();
            for getter in getters {
                if let Some(v) = getter(lua, &ctx, &this.id)? {
                    return Ok(v);
                }
            }
            let keyed_getters = ctx
                .borrow()
                .keyed_getters
                .get(&key)
                .cloned()
                .unwrap_or_default();
            for getter in keyed_getters {
                if let Some(v) = getter(lua, &ctx, &this.id, &key)? {
                    return Ok(v);
                }
            }
            if is_signal_name(&key) {
                return super::signals::signal(lua, &ctx, &this.id, &key);
            }
            if received_event_signature(&ctx, &class, &key) {
                return super::signals::signal(lua, &ctx, &this.id, &key);
            }
            super::instance_helpers::ensure_property_scriptable(lua, &ctx, &this.id, &raw_key)?;
            let property_key = super::instance_helpers::canonical_hidden_property(&class, &key);
            match get_property(&ctx, &this.id, &property_key) {
                Ok(json) => value_from_json(lua, &ctx, &json),
                Err(error) => {
                    // Roblox also resolves direct children by name, after members.
                    let child = {
                        let world = ctx.borrow();
                        world.nodes.child_named(&this.id, &key, &world.node_order)
                    };
                    match child {
                        Some(id) => instance(lua, &ctx, &id),
                        None => Err(error),
                    }
                }
            }
        });
        methods.add_meta_method_mut(
            MetaMethod::NewIndex,
            |lua, this, (raw_key, value): (String, Value)| {
                let ctx = this.ctx()?;
                let class = ctx
                    .borrow()
                    .nodes
                    .get(&this.id)
                    .map(|n| n.class_name.clone())
                    .unwrap_or_default();
                let key = super::instance_helpers::canonical_hidden_property(
                    &class,
                    &canonical_member(&class, &raw_key),
                );
                if matches!(key.as_str(), "ClassName") {
                    return Err(mlua::Error::RuntimeError("ClassName is read-only".into()));
                }
                if class == "Player" {
                    if key == "GameplayPaused"
                        && !super::scheduler::has_capability(lua, "RobloxScript")
                    {
                        return Err(mlua::Error::RuntimeError(
                            "GameplayPaused: lacking capability RobloxScript".into(),
                        ));
                    }
                    if matches!(key.as_str(), "Name" | "UserId")
                        && !super::scheduler::has_capability(lua, "WritePlayer")
                    {
                        return Err(mlua::Error::RuntimeError(
                            "Player identity: lacking capability WritePlayer".into(),
                        ));
                    }
                }
                super::instance_helpers::ensure_property_scriptable(lua, &ctx, &this.id, &raw_key)?;
                if let Some(enum_type) = local_part_enum_type(&class, &key) {
                    let Value::UserData(item) = &value else {
                        return Err(mlua::Error::RuntimeError(format!(
                            "{key} requires Enum.{enum_type} item"
                        )));
                    };
                    if !super::enums::enum_item_matches(item, enum_type)? {
                        return Err(mlua::Error::RuntimeError(format!(
                            "{key} requires Enum.{enum_type} item"
                        )));
                    }
                }
                let setters = ctx.borrow().setters.get(&key).cloned().unwrap_or_default();
                for setter in setters {
                    if setter(lua, &ctx, &this.id, value.clone())? {
                        return Ok(());
                    }
                }
                let keyed_setters = ctx
                    .borrow()
                    .keyed_setters
                    .get(&key)
                    .cloned()
                    .unwrap_or_default();
                for setter in keyed_setters {
                    if setter(lua, &ctx, &this.id, &key, value.clone())? {
                        return Ok(());
                    }
                }
                let json = super::values::to_json(lua, &value)?;
                set_property(lua, &ctx, &this.id, &key, json)
            },
        );
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            let ctx = this.ctx()?;
            let world = ctx.borrow();
            let n = world.nodes.get(&this.id);
            Ok(
                n.map(|n| n.name.clone())
                    .unwrap_or_else(|| "<destroyed Instance>".into()),
            )
        });
        methods.add_method("GetChildren", |lua, this, ()| {
            let ctx = this.ctx()?;
            let ids = children(&ctx, &this.id);
            let table = lua.create_table_with_capacity(ids.len(), 0)?;
            for (i, id) in ids.iter().enumerate() {
                table.raw_set(i + 1, instance(lua, &ctx, id)?)?;
            }
            Ok(table)
        });
        methods.add_method("GetDescendants", |lua, this, ()| {
            let ctx = this.ctx()?;
            let out = descendants(&ctx, &this.id);
            if out.len() > 400_000 {
                return Err(mlua::Error::RuntimeError(
                    "descendant limit exceeded".into(),
                ));
            }
            let table = lua.create_table_with_capacity(out.len(), 0)?;
            for (i, id) in out.iter().enumerate() {
                table.raw_set(i + 1, instance(lua, &ctx, id)?)?;
            }
            Ok(table)
        });
        methods.add_method("QueryDescendants", |lua, this, selector: String| {
            let ctx = this.ctx()?;
            world_queries::query(lua, &ctx, &this.id, &selector)
        });
        methods.add_method("GetActor", |lua, this, ()| {
            let ctx = this.ctx()?;
            let world = ctx.borrow();
            let mut current = Some(this.id.clone());
            let mut seen = std::collections::HashSet::new();
            let mut found = None;
            while let Some(id) = current {
                if !seen.insert(id.clone()) {
                    break;
                }
                let Some(node) = world.nodes.get(&id).filter(|node| !node.destroyed) else {
                    break;
                };
                if class_chain(&node.class_name)
                    .iter()
                    .any(|class| class == "Actor")
                {
                    found = Some(id);
                    break;
                }
                current = node.parent.clone();
            }
            drop(world);
            match found {
                Some(id) => instance(lua, &ctx, &id),
                None => Ok(Value::Nil),
            }
        });
        methods.add_method("IsPropertyModified", |lua, this, raw_key: String| {
            let ctx = this.ctx()?;
            let class = ctx
                .borrow()
                .nodes
                .get(&this.id)
                .map(|node| node.class_name.clone())
                .ok_or_else(|| mlua::Error::RuntimeError("instance is not in this world".into()))?;
            let key = canonical_member(&class, &raw_key);
            let (default, _) = constructor_default(&class, &key).map_err(|error| {
                mlua::Error::RuntimeError(format!("IsPropertyModified failed: {error}"))
            })?;
            let Value::UserData(object) = instance(lua, &ctx, &this.id)? else {
                return Err(mlua::Error::RuntimeError(
                    "IsPropertyModified failed: instance is unavailable".into(),
                ));
            };
            let current: Value = object.get(key.as_str()).map_err(|error| {
                mlua::Error::RuntimeError(format!("IsPropertyModified failed: {error}"))
            })?;
            let expected = value_from_json(lua, &ctx, &default).map_err(|error| {
                mlua::Error::RuntimeError(format!("IsPropertyModified failed: {error}"))
            })?;
            let current = super::values::to_json(lua, &current).map_err(|error| {
                mlua::Error::RuntimeError(format!("IsPropertyModified failed: {error}"))
            })?;
            let expected = super::values::to_json(lua, &expected).map_err(|error| {
                mlua::Error::RuntimeError(format!("IsPropertyModified failed: {error}"))
            })?;
            Ok(!json_property_equal(&current, &expected))
        });
        methods.add_method("ResetPropertyToDefault", |lua, this, raw_key: String| {
            let ctx = this.ctx()?;
            let class = ctx
                .borrow()
                .nodes
                .get(&this.id)
                .map(|node| node.class_name.clone())
                .ok_or_else(|| mlua::Error::RuntimeError("instance is not in this world".into()))?;
            let key = canonical_member(&class, &raw_key);
            let (default, writable) = constructor_default(&class, &key).map_err(|error| {
                mlua::Error::RuntimeError(format!("ResetPropertyToDefault failed: {error}"))
            })?;
            if !writable {
                return Err(mlua::Error::RuntimeError(
                    "ResetPropertyToDefault failed: property is read-only".into(),
                ));
            }
            let value = value_from_json(lua, &ctx, &default).map_err(|error| {
                mlua::Error::RuntimeError(format!("ResetPropertyToDefault failed: {error}"))
            })?;
            let setters = ctx.borrow().setters.get(&key).cloned().unwrap_or_default();
            for setter in setters {
                if setter(lua, &ctx, &this.id, value.clone())? {
                    return Ok(());
                }
            }
            let keyed_setters = ctx
                .borrow()
                .keyed_setters
                .get(&key)
                .cloned()
                .unwrap_or_default();
            for setter in keyed_setters {
                if setter(lua, &ctx, &this.id, &key, value.clone())? {
                    return Ok(());
                }
            }
            let value = super::values::to_json(lua, &value).map_err(|error| {
                mlua::Error::RuntimeError(format!("ResetPropertyToDefault failed: {error}"))
            })?;
            set_property(lua, &ctx, &this.id, &key, value).map_err(|error| {
                mlua::Error::RuntimeError(format!("ResetPropertyToDefault failed: {error}"))
            })
        });
        methods.add_method(
            "FindFirstChild",
            |lua, this, (name, recursive): (String, Option<bool>)| {
                let ctx = this.ctx()?;
                let candidates = if recursive.unwrap_or(false) {
                    descendants(&ctx, &this.id)
                } else {
                    children(&ctx, &this.id)
                };
                let id = candidates
                    .into_iter()
                    .find(|id| ctx.borrow().nodes.get(id).is_some_and(|n| n.name == name));
                match id {
                    Some(id) => instance(lua, &ctx, &id),
                    None => Ok(Value::Nil),
                }
            },
        );
        methods.add_method(
            "FindFirstChildOfClass",
            |lua, this, (class, recursive): (String, Option<bool>)| {
                let ctx = this.ctx()?;
                let candidates = if recursive.unwrap_or(false) {
                    descendants(&ctx, &this.id)
                } else {
                    children(&ctx, &this.id)
                };
                let id = candidates.into_iter().find(|id| {
                    ctx.borrow()
                        .nodes
                        .get(id)
                        .is_some_and(|n| n.class_name == class)
                });
                match id {
                    Some(id) => instance(lua, &ctx, &id),
                    None => Ok(Value::Nil),
                }
            },
        );
        methods.add_method(
            "FindFirstChildWhichIsA",
            |lua, this, (class, recursive): (String, Option<bool>)| {
                let ctx = this.ctx()?;
                let candidates = if recursive.unwrap_or(false) {
                    descendants(&ctx, &this.id)
                } else {
                    children(&ctx, &this.id)
                };
                let id = candidates.into_iter().find(|id| is_a(&ctx, id, &class));
                match id {
                    Some(id) => instance(lua, &ctx, &id),
                    None => Ok(Value::Nil),
                }
            },
        );
        for method in ["GetService", "service"] {
            methods.add_method(method, |lua, this, name: String| {
                let ctx = this.ctx()?;
                let (receiver_class, receiver_is_game, receiver_is_settings, mapped) = {
                    let world = ctx.borrow();
                    let node = world.nodes.get(&this.id).ok_or_else(|| {
                        mlua::Error::RuntimeError("instance is not in this world".into())
                    })?;
                    let game_id = world
                        .nodes
                        .first_live_of_class("DataModel")
                        .map(|n| n.id.as_str());
                    (
                        node.class_name.clone(),
                        game_id == Some(this.id.as_str()),
                        node.class_name == "UserSettings",
                        world.services.get(&name).cloned(),
                    )
                };
                if receiver_is_settings {
                    if name != "UserGameSettings" {
                        return Err(mlua::Error::RuntimeError(
                            "unknown user settings service".into(),
                        ));
                    }
                    let id = mapped.ok_or_else(|| {
                        mlua::Error::RuntimeError("UserGameSettings model unavailable".into())
                    })?;
                    return instance(lua, &ctx, &id);
                }
                if !receiver_is_game {
                    return Err(mlua::Error::RuntimeError(
                        "GetService requires ServiceProvider".into(),
                    ));
                }
                if let Some(id) = mapped {
                    if !ctx.borrow().nodes.contains_key(&id) {
                        return Err(mlua::Error::RuntimeError(
                            "snapshot service reference is invalid".into(),
                        ));
                    }
                    return instance(lua, &ctx, &id);
                }
                if local_service_supported(&name) {
                    return ensure_client_service(lua, &ctx, &name);
                }
                let _ = receiver_class;
                Err(mlua::Error::RuntimeError(format!(
                    "service unavailable in snapshot: {name}"
                )))
            });
        }
        methods.add_method("FindService", |lua, this, name: String| {
            let ctx = this.ctx()?;
            let (receiver_is_game, receiver_is_settings, mapped, local_id) = {
                let world = ctx.borrow();
                let node = world.nodes.get(&this.id).ok_or_else(|| {
                    mlua::Error::RuntimeError("instance is not in this world".into())
                })?;
                let game_id = world
                    .nodes
                    .first_live_of_class("DataModel")
                    .map(|n| n.id.as_str());
                let mapped = world.services.get(&name).cloned();
                let local_id = if mapped.is_none() && local_service_supported(&name) {
                    world.node_order.iter().find_map(|id| {
                        if world.nodes.class_name(id) != Some(name.as_str()) {
                            return None;
                        }
                        world
                            .nodes
                            .get(id)
                            .filter(|n| !n.server_owned && !n.destroyed && n.class_name == name)
                            .map(|n| n.id.clone())
                    })
                } else {
                    None
                };
                (
                    game_id == Some(this.id.as_str()),
                    node.class_name == "UserSettings",
                    mapped,
                    local_id,
                )
            };
            if receiver_is_settings {
                if name != "UserGameSettings" {
                    return Ok(Value::Nil);
                }
                return match mapped {
                    Some(id) => instance(lua, &ctx, &id),
                    None => Ok(Value::Nil),
                };
            }
            if !receiver_is_game {
                return Err(mlua::Error::RuntimeError(
                    "FindService requires ServiceProvider".into(),
                ));
            }
            let id = mapped.or(local_id);
            match id {
                Some(id) => instance(lua, &ctx, &id),
                None => Ok(Value::Nil),
            }
        });
        methods.add_method("GetFullName", |_, this, ()| {
            let ctx = this.ctx()?;
            let world = ctx.borrow();
            instance_full_name(&world, &this.id)
        });
        methods.add_method("IsDescendantOf", |_, this, other: AnyUserData| {
            let ctx = this.ctx()?;
            let other_value = Value::UserData(other.clone());
            if !instance_belongs_to(&other_value, &ctx) {
                return Err(mlua::Error::RuntimeError(
                    "argument belongs to a different world snapshot".into(),
                ));
            }
            let parent = other
                .borrow::<InstanceHandle>()
                .map_err(|_| mlua::Error::RuntimeError("expected Instance".into()))?
                .id
                .clone();
            let world = ctx.borrow();
            let mut current = world.nodes.get(&this.id).and_then(|n| n.parent.clone());
            let mut seen = 0;
            while let Some(id) = current {
                if id == parent {
                    return Ok(true);
                }
                current = world.nodes.get(&id).and_then(|n| n.parent.clone());
                seen += 1;
                if seen > 400_000 {
                    break;
                }
            }
            Ok(false)
        });
        methods.add_method("IsAncestorOf", |_, this, other: AnyUserData| {
            let ctx = this.ctx()?;
            let other_value = Value::UserData(other.clone());
            if !instance_belongs_to(&other_value, &ctx) {
                return Err(mlua::Error::RuntimeError(
                    "argument belongs to a different world snapshot".into(),
                ));
            }
            let descendant = other
                .borrow::<InstanceHandle>()
                .map_err(|_| mlua::Error::RuntimeError("expected Instance".into()))?
                .id
                .clone();
            let world = ctx.borrow();
            let mut current = world.nodes.get(&descendant).and_then(|n| n.parent.clone());
            let mut seen = 0;
            while let Some(id) = current {
                if id == this.id {
                    return Ok(true);
                }
                current = world.nodes.get(&id).and_then(|n| n.parent.clone());
                seen += 1;
                if seen > 400_000 {
                    break;
                }
            }
            Ok(false)
        });
        methods.add_method("Destroy", |lua, this, ()| {
            destroy_instance(lua, &this.ctx()?, &this.id)
        });
        methods.add_method("ClearAllChildren", |lua, this, ()| {
            let ctx = this.ctx()?;
            for child in children(&ctx, &this.id) {
                destroy_instance(lua, &ctx, &child)?;
            }
            Ok(())
        });
        methods.add_method("Clone", |lua, this, ()| {
            clone_instance(lua, &this.ctx()?, &this.id)
        });
        methods.add_method("Remove", |lua, this, ()| {
            set_property(lua, &this.ctx()?, &this.id, "Parent", Json::Null)
        });
        methods.add_method("IsA", |_, this, class: String| {
            Ok(is_a(&this.ctx()?, &this.id, &class))
        });
        methods.add_method("FindFirstAncestor", |lua, this, name: String| {
            let ctx = this.ctx()?;
            let world = ctx.borrow();
            let mut current = world
                .nodes
                .get(&this.id)
                .and_then(|node| node.parent.clone());
            let mut seen = std::collections::HashSet::new();
            let mut found = None;
            while let Some(id) = current {
                if !seen.insert(id.clone()) {
                    break;
                }
                let Some(node) = world.nodes.get(&id) else {
                    break;
                };
                if node.name == name {
                    found = Some(id);
                    break;
                }
                current = node.parent.clone();
            }
            drop(world);
            match found {
                Some(id) => instance(lua, &ctx, &id),
                None => Ok(Value::Nil),
            }
        });
        methods.add_method("FindFirstAncestorOfClass", |lua, this, class: String| {
            let ctx = this.ctx()?;
            let world = ctx.borrow();
            let mut current = world
                .nodes
                .get(&this.id)
                .and_then(|node| node.parent.clone());
            let mut seen = std::collections::HashSet::new();
            let mut found = None;
            while let Some(id) = current {
                if !seen.insert(id.clone()) {
                    break;
                }
                let Some(node) = world.nodes.get(&id) else {
                    break;
                };
                if node.class_name == class {
                    found = Some(id);
                    break;
                }
                current = node.parent.clone();
            }
            drop(world);
            match found {
                Some(id) => instance(lua, &ctx, &id),
                None => Ok(Value::Nil),
            }
        });
        methods.add_method("FindFirstAncestorWhichIsA", |lua, this, class: String| {
            let ctx = this.ctx()?;
            let world = ctx.borrow();
            let mut current = world
                .nodes
                .get(&this.id)
                .and_then(|node| node.parent.clone());
            let mut seen = std::collections::HashSet::new();
            let mut found = None;
            while let Some(id) = current {
                if !seen.insert(id.clone()) {
                    break;
                }
                let Some(node) = world.nodes.get(&id) else {
                    break;
                };
                if class_chain(&node.class_name)
                    .iter()
                    .any(|ancestor| ancestor == &class)
                {
                    found = Some(id);
                    break;
                }
                current = node.parent.clone();
            }
            drop(world);
            match found {
                Some(id) => instance(lua, &ctx, &id),
                None => Ok(Value::Nil),
            }
        });
        methods.add_method("GetAttribute", |lua, this, name: String| {
            let ctx = this.ctx()?;
            let (value, known, class_name) = {
                let world = ctx.borrow();
                let node = world.nodes.get(&this.id).ok_or_else(|| {
                    mlua::Error::RuntimeError("instance is not in this world".into())
                })?;
                (
                    node.attributes.get(&name).cloned(),
                    node.attributes_known,
                    node.class_name.clone(),
                )
            };
            if let Some(value) = value {
                return value_from_json(lua, &ctx, &value);
            }
            if !known {
                return Err(mlua::Error::RuntimeError(format!(
                    "attributes unavailable in snapshot for {}",
                    class_name
                )));
            }
            Ok(Value::Nil)
        });
        methods.add_method("GetPropertyChangedSignal", |lua, this, property: String| {
            super::signals::signal(lua, &this.ctx()?, &this.id, &format!("property:{property}"))
        });
        methods.add_method(
            "GetAttributeChangedSignal",
            |lua, this, attribute: String| {
                super::signals::signal(
                    lua,
                    &this.ctx()?,
                    &this.id,
                    &format!("attribute:{attribute}"),
                )
            },
        );
        methods.add_method("GetAttributes", |lua, this, ()| {
            let ctx = this.ctx()?;
            let (attributes, known) = {
                let world = ctx.borrow();
                let node = world.nodes.get(&this.id).ok_or_else(|| {
                    mlua::Error::RuntimeError("instance is not in this world".into())
                })?;
                (node.attributes.clone(), node.attributes_known)
            };
            if !known {
                return Err(mlua::Error::RuntimeError(format!(
                    "attributes unavailable in snapshot for {}",
                    ctx.borrow().nodes[&this.id].class_name
                )));
            }
            let table = lua.create_table_with_capacity(attributes.len(), 0)?;
            for (key, value) in &attributes {
                table.set(key.as_str(), value_from_json(lua, &ctx, value)?)?;
            }
            Ok(table)
        });
        methods.add_method(
            "SetAttribute",
            |lua, this, (name, value): (String, Value)| {
                if name.is_empty()
                    || name.len() > 100
                    || name.starts_with("RBX")
                    || !name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "_./-".contains(c))
                {
                    return Err(mlua::Error::RuntimeError(
                        "invalid or reserved attribute name".into(),
                    ));
                }
                let ctx = this.ctx()?;
                let mut json = super::values::to_json(lua, &value)?;
                if let Some(id) = instance_id(&value) {
                    json = json!({"$instance_handle":id});
                }
                let mut world = ctx.borrow_mut();
                let actor_internal_revision = world.local_namespace.starts_with("local:actor:");
                let node = world.nodes.get_mut(&this.id).ok_or_else(|| {
                    mlua::Error::RuntimeError("instance is not in this world".into())
                })?;
                let base_local_revision = node.local_revision;
                let mut attributes = crate::engine_attributes::AttributeStore::from_snapshot(
                    Json::Object(node.attributes.clone()),
                    node.attributes_known,
                )
                .map_err(|error| mlua::Error::RuntimeError(error.to_string()))?;
                let changed = attributes
                    .set(
                        &name,
                        if matches!(value, Value::Nil) {
                            None
                        } else {
                            Some(json.clone())
                        },
                    )
                    .map_err(|error| mlua::Error::RuntimeError(error.to_string()))?;
                node.attributes = attributes
                    .snapshot()
                    .as_object()
                    .cloned()
                    .unwrap_or_default();
                node.attributes_known = attributes.is_known();
                if changed {
                    world.local_revision = world.local_revision.wrapping_add(1).max(1);
                    let revision = world.local_revision;
                    if let Some(node) = world.nodes.get_mut(&this.id) {
                        node.local_revision = revision;
                    }
                }
                drop(world);
                let mut mutation =
                    json!({"kind":"attribute","id":this.id,"name":name,"value":json});
                if actor_internal_revision {
                    mutation["_base_local_revision"] = json!(base_local_revision);
                }
                emit(&ctx, "mutations", mutation);
                if changed {
                    // Actor VMs journal only the attribute action; the holder
                    // create would need its own fencing, so it is built when
                    // the write reaches the root world instead.
                    if !actor_internal_revision {
                        let current = {
                            let world = ctx.borrow();
                            world
                                .nodes
                                .get(&this.id)
                                .and_then(|n| n.attributes.get(&name))
                                .cloned()
                        };
                        sync_attribute_child(lua, &ctx, &this.id, &name, current.as_ref())?;
                    }
                    fire_attribute_changed(lua, &ctx, &this.id, &name)?;
                }
                Ok(())
            },
        );
    }
}

fn is_signal_name(key: &str) -> bool {
    matches!(
        key,
        "Changed"
            | "AttributeChanged"
            | "Loaded"
            | "ChildAdded"
            | "ChildRemoved"
            | "DescendantAdded"
            | "DescendantRemoving"
            | "AncestryChanged"
            | "Destroying"
            | "Touched"
            | "TouchStarted"
            | "TouchMoved"
            | "TouchEnded"
            | "TouchLongPress"
            | "TouchTap"
            | "TouchPan"
            | "ComputerCameraMovementModeRegistered"
            | "ComputerMovementModeRegistered"
            | "TouchCameraMovementModeRegistered"
            | "TouchMovementModeRegistered"
            | "RecordingCompleted"
            | "PlaybackCompleted"
            | "Heartbeat"
            | "Stepped"
            | "RenderStepped"
            | "PreRender"
            | "PreAnimation"
            | "PreSimulation"
            | "PostSimulation"
            | "CharacterAdded"
            | "CharacterRemoving"
            | "CharacterAppearanceLoaded"
            | "PlayerAdded"
            | "PlayerRemoving"
            | "InputBegan"
            | "InputChanged"
            | "InputEnded"
            | "MouseButton1Click"
            | "MouseButton1Down"
            | "MouseButton1Up"
            | "MouseEnter"
            | "MouseLeave"
            | "Activated"
            | "Focused"
            | "FocusLost"
            | "TextChanged"
            | "OnClientEvent"
            | "OnServerEvent"
            | "LastInputTypeChanged"
            | "WindowFocused"
            | "WindowFocusReleased"
            | "TextBoxFocused"
            | "TextBoxFocusReleased"
            | "Button1Down"
            | "Button1Up"
            | "Button2Down"
            | "Button2Up"
            | "Move"
            | "WheelForward"
            | "WheelBackward"
    )
}

fn received_event_signature(ctx: &WorldRef, class: &str, event: &str) -> bool {
    let world = ctx.borrow();
    let signatures = &world.snapshot["class_event_signatures"];
    for class in class_chain(class) {
        if signatures[&class][event].as_array().is_some() {
            return true;
        }
    }
    false
}

fn refresh_globals(lua: &Lua, ctx: &WorldRef) -> mlua::Result<()> {
    world_globals::refresh(lua, ctx, None, true).map(|_| ())
}

pub fn install(lua: &Lua, ctx: &WorldRef) -> mlua::Result<()> {
    lua.set_app_data(Rc::downgrade(ctx));
    super::player_lifecycle::install(lua, ctx);
    if lua.app_data_ref::<InstanceHandleWaiters>().is_none() {
        lua.set_app_data(Rc::new(RefCell::new(InstanceHandleWaitState::default())));
    }
    if lua.app_data_ref::<super::signals::SignalHubRef>().is_none() {
        super::signals::install(lua, ctx)?;
    }
    super::values::register_codec(
        lua,
        super::values::Codec {
            key: "$ref",
            type_name: "Instance",
            decode: decode_instance_ref,
            encode: encode_instance_ref,
        },
    )?;
    super::values::register_codec(
        lua,
        super::values::Codec {
            key: "$instance_handle",
            type_name: "InstanceHandle",
            decode: decode_instance_handle,
            encode: encode_instance_handle,
        },
    )?;
    let globals = lua.globals();
    refresh_globals(lua, ctx)?;
    for service in ["UserInputService", "RunService", "CoreGui", "CorePackages"] {
        if !ctx.borrow().services.contains_key(service) {
            ensure_client_service(lua, ctx, service)?;
        }
    }
    let instance_table = lua.create_table()?;
    let weak = Rc::downgrade(ctx);
    instance_table.set(
        "new",
        lua.create_function(move |lua, (class, parent): (String, Option<AnyUserData>)| {
            let ctx = weak
                .upgrade()
                .ok_or_else(|| mlua::Error::RuntimeError("world has been released".into()))?;
            let parent_id = if let Some(parent) = parent {
                let parent = parent
                    .borrow::<InstanceHandle>()
                    .map_err(|_| mlua::Error::RuntimeError("Parent must be an Instance".into()))?;
                let other = parent
                    .world
                    .upgrade()
                    .ok_or_else(|| mlua::Error::RuntimeError("world has been released".into()))?;
                if !Rc::ptr_eq(&other, &ctx) {
                    return Err(mlua::Error::RuntimeError(
                        "Parent belongs to a different world snapshot".into(),
                    ));
                }
                Some(parent.id.clone())
            } else {
                None
            };
            create_instance(lua, &ctx, &class, parent_id.as_deref())
        })?,
    )?;
    globals.set("Instance", instance_table)?;
    let handle_table = lua.create_table()?;
    let weak = Rc::downgrade(ctx);
    handle_table.set(
        "new",
        lua.create_function(move |lua, value: Option<Value>| {
            let ctx = weak
                .upgrade()
                .ok_or_else(|| mlua::Error::RuntimeError("world has been released".into()))?;
            let id = match value.unwrap_or(Value::Nil) {
                Value::Nil => String::new(),
                Value::UserData(data) => {
                    let handle = data.borrow::<InstanceHandle>().map_err(|_| {
                        mlua::Error::RuntimeError("expected an Instance or nil".into())
                    })?;
                    let other = handle.world.upgrade().ok_or_else(|| {
                        mlua::Error::RuntimeError("world has been released".into())
                    })?;
                    if !Rc::ptr_eq(&other, &ctx) {
                        return Err(mlua::Error::RuntimeError(
                            "Instance belongs to a different snapshot".into(),
                        ));
                    }
                    handle.id.clone()
                }
                _ => {
                    return Err(mlua::Error::RuntimeError(
                        "expected an Instance or nil".into(),
                    ));
                }
            };
            Ok(Value::UserData(lua.create_userdata(InstanceRefValue {
                id,
                world: Rc::downgrade(&ctx),
            })?))
        })?,
    )?;
    handle_table.set_readonly(true);
    globals.set("InstanceHandle", handle_table)?;
    let client: mlua::Table = globals.get("client").unwrap_or(lua.create_table()?);
    {
        let world = ctx.borrow();
        let persistent = world
            .snapshot
            .get("persistent")
            .and_then(Json::as_bool)
            .unwrap_or(false);
        client.set("snapshot", !persistent)?;
        client.set("persistent", persistent)?;
        if world.sequence > 0 {
            client.set("sequence", world.sequence)?;
        }
    }
    let weak = Rc::downgrade(ctx);
    client.set(
        "propertyOrigin",
        lua.create_function(move |_, (object, property): (Value, String)| {
            let ctx = weak
                .upgrade()
                .ok_or_else(|| mlua::Error::RuntimeError("world has been released".into()))?;
            let id = instance_id(&object)
                .filter(|_| instance_belongs_to(&object, &ctx))
                .ok_or_else(|| {
                    mlua::Error::RuntimeError(
                        "propertyOrigin expects an Instance from this world".into(),
                    )
                })?;
            let class = ctx
                .borrow()
                .nodes
                .get(&id)
                .map(|node| node.class_name.clone())
                .ok_or_else(|| mlua::Error::RuntimeError("instance is not in this world".into()))?;
            let key = canonical_member(&class, &property);
            property_origin(&ctx, &id, &key)
        })?,
    )?;
    globals.set("client", client)?;
    Ok(())
}
