//! Most received instances have no local fields. Allocate those only on write.
use super::{Json, JsonMap, Node};
use std::ops::{Deref, DerefMut};
use std::sync::OnceLock;

#[derive(Clone, Default)]
pub struct NodeData {
    pub properties: JsonMap<String, Json>,
    pub local_overrides: JsonMap<String, Json>,
    pub attributes: JsonMap<String, Json>,
    pub revisions: JsonMap<String, Json>,
    pub local_revision: u64,
    pub collection_tags: Vec<String>,
}

impl NodeData {
    pub fn pack(self) -> Option<Box<Self>> {
        (!self.is_empty()).then(|| Box::new(self))
    }
    fn is_empty(&self) -> bool {
        self.properties.is_empty()
            && self.local_overrides.is_empty()
            && self.attributes.is_empty()
            && self.revisions.is_empty()
            && self.local_revision == 0
            && self.collection_tags.is_empty()
    }
}

impl Deref for Node {
    type Target = NodeData;
    fn deref(&self) -> &NodeData {
        static EMPTY: OnceLock<NodeData> = OnceLock::new();
        self.data
            .as_deref()
            .unwrap_or_else(|| EMPTY.get_or_init(NodeData::default))
    }
}

impl DerefMut for Node {
    fn deref_mut(&mut self) -> &mut NodeData {
        self.data
            .get_or_insert_with(|| Box::new(NodeData::default()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn absent_state_reads_and_clone_writes_stay_isolated() {
        let snapshot = json!({"version":1,"nodes":[{"id":"part","class_name":"Part","name":"Part","parent":null}],"services":{}});
        let world = super::super::World::from_snapshot(&snapshot, None).unwrap();
        let node = world.nodes.get("part").unwrap();
        assert!(node.data.is_none());
        assert!(node.properties.is_empty());
        assert!(node.attributes.is_empty());
        assert_eq!(node.local_revision, 0);
        assert!(
            node.data.is_none(),
            "reading an empty field must not allocate"
        );
        let mut first = node.clone();
        first.attributes.insert("score".into(), json!(1));
        let mut second = first.clone();
        second.attributes.insert("score".into(), json!(2));
        assert_eq!(first.attributes["score"], 1);
        assert_eq!(second.attributes["score"], 2);
        assert!(node.data.is_none());
        assert!(NodeData::default().pack().is_none());
    }
    #[test]
    fn shared_initialization_reads_received_revisions_without_eager_maps() {
        use crate::{
            engine_world_projection::ProjectionContext, engine_world_view::SharedWorldView,
        };
        use rove_client_core::{schema::Schema, world::WorldStore};
        use std::{cell::RefCell, collections::HashMap, rc::Rc};
        let mut store =
            WorldStore::from_snapshot(&json!({"version":1,"sequence":0,"nodes":[]})).unwrap();
        store.apply_delta(&json!({"version":1,"delta":true,"sequence":5,"actions":[
            {"kind":"create","sequence":5,"node":{"id":"p","class_name":"Part","name":"Part","parent":null,"properties":{"Anchored":false}}}
        ]})).unwrap();
        let schema = Schema {
            version: 3,
            flags: 0,
            enums: vec![],
            classes: vec![],
            properties: vec![],
            events: vec![],
            strings: vec![],
            names: vec![],
            lr_types: vec![],
            parent_property_id: 0,
            decoded_bytes: 0,
        };
        let context = ProjectionContext::new(&schema, HashMap::new()).unwrap();
        let view = SharedWorldView::projected(store.view(), context);
        let world = Rc::new(RefCell::new(
            super::super::World::from_snapshot(
                &json!({"version":1,"sequence":5,"nodes":[]}),
                Some(&view),
            )
            .unwrap(),
        ));
        assert!(world.borrow().nodes["p"].data.is_none());
        assert_eq!(
            super::super::get_property_revision(&world, "p", "Anchored"),
            Some(5)
        );
        assert_eq!(
            super::super::get_property(&world, "p", "Anchored").unwrap(),
            json!(false)
        );
        assert_eq!(
            super::super::get_property_revision(&world, "p", "missing"),
            None
        );
        assert!(world.borrow().nodes["p"].data.is_none());
        let lua = mlua::Lua::new();
        crate::rust_host::signals::install(&lua, &world).unwrap();
        super::super::set_property(&lua, &world, "p", "Anchored", json!(true)).unwrap();
        assert_eq!(
            super::super::get_property(&world, "p", "Anchored").unwrap(),
            json!(true)
        );
        assert!(world.borrow().nodes["p"].data.is_some());
        assert_eq!(
            super::super::get_property_revision(&world, "p", "Anchored"),
            Some(5)
        );
    }

    #[test]
    fn untouched_shared_instances_stay_lazy_through_class_and_child_queries() {
        use crate::{
            engine_world_projection::ProjectionContext, engine_world_view::SharedWorldView,
        };
        use rove_client_core::{schema::Schema, world::WorldStore};
        use std::{cell::RefCell, collections::HashMap, rc::Rc};
        let store = WorldStore::from_snapshot(&json!({"version":1,"nodes":[
            {"id":"game","class_name":"DataModel","name":"game","parent":null},
            {"id":"p","class_name":"Part","name":"first","parent":"game","properties":{"Anchored":false}},
            {"id":"q","class_name":"Part","name":"second","parent":"game","properties":{"Anchored":true}}
        ]})).unwrap();
        let schema = Schema {
            version: 3,
            flags: 0,
            enums: vec![],
            classes: vec![],
            properties: vec![],
            events: vec![],
            strings: vec![],
            names: vec![],
            lr_types: vec![],
            parent_property_id: 0,
            decoded_bytes: 0,
        };
        let view = SharedWorldView::projected(
            store.view(),
            ProjectionContext::new(&schema, HashMap::new()).unwrap(),
        );
        let world = Rc::new(RefCell::new(
            super::super::World::from_snapshot(&json!({"version":1,"nodes":[]}), Some(&view))
                .unwrap(),
        ));
        assert_eq!(
            world.borrow().nodes.materialized_count(),
            1,
            "only the DataModel is needed during bootstrap"
        );
        assert!(world.borrow().nodes.contains_key("q"));
        assert_eq!(super::super::children(&world, "game"), vec!["p", "q"]);
        assert_eq!(world.borrow().nodes.of_class("DataModel").count(), 1);
        assert!(super::super::is_a(&world, "q", "BasePart"));
        assert!(!super::super::is_a(&world, "q", "Actor"));
        let lua = mlua::Lua::new();
        let pool = crate::rust_host::shared_tables::SharedTableRegistry::default();
        crate::rust_host::actors::install(&lua, &world, pool.clone()).unwrap();
        crate::rust_host::actors::step(&lua, &world, &pool, std::time::Duration::from_millis(5))
            .unwrap();
        crate::rust_host::remotes::install(&lua, &world).unwrap();
        crate::rust_host::remotes::cleanup_destroyed_instances(&lua, &world).unwrap();
        super::super::apply_delta_with_lua(
            &lua,
            &world,
            &json!({"version":1,"delta":true,"sequence":1,"actions":[]}),
            None,
        )
        .unwrap();
        assert_eq!(
            world.borrow().nodes.materialized_count(),
            1,
            "metadata and per-tick class scans must not create all wrappers"
        );
        assert_eq!(
            super::super::get_property(&world, "p", "Anchored").unwrap(),
            json!(false)
        );
        assert_eq!(world.borrow().nodes.materialized_count(), 2);
        let fork = Rc::new(RefCell::new(world.borrow().fork_for_vm()));
        fork.borrow_mut().nodes.get_mut("q").unwrap().parent = Some("p".into());
        assert_eq!(super::super::children(&fork, "game"), vec!["p"]);
        assert_eq!(super::super::children(&world, "game"), vec!["p", "q"]);
        assert_eq!(world.borrow().nodes.materialized_count(), 2);
        let mut sidecar = world.borrow().nodes["p"].clone();
        sidecar.name = "local name".into();
        world.borrow_mut().nodes.insert("p".into(), sidecar);
        assert_eq!(
            world.borrow().nodes["p"].name,
            "local name",
            "explicit header overrides stay materialized"
        );
    }

    #[test]
    fn received_create_and_destroy_preserve_lazy_nodes_and_hierarchy_events() {
        use crate::rust_host::{scheduler, world as host};
        use crate::{
            engine_world_projection::ProjectionContext, engine_world_view::SharedWorldView,
        };
        use rove_client_core::{schema::Schema, world::WorldStore};
        use std::{
            cell::RefCell,
            collections::HashMap,
            rc::Rc,
            time::{Duration, Instant},
        };
        let mut store = WorldStore::from_snapshot(&json!({"version":1,"nodes":[
            {"id":"game","class_name":"DataModel","name":"game","parent":null},
            {"id":"p","class_name":"Part","name":"first","parent":"game","properties":{}},
            {"id":"q","class_name":"Part","name":"second","parent":"game","properties":{}}
        ]}))
        .unwrap();
        let schema = Schema {
            version: 3,
            flags: 0,
            enums: vec![],
            classes: vec![],
            properties: vec![],
            events: vec![],
            strings: vec![],
            names: vec![],
            lr_types: vec![],
            parent_property_id: 0,
            decoded_bytes: 0,
        };
        let context = ProjectionContext::new(&schema, HashMap::new()).unwrap();
        let view = SharedWorldView::projected(store.view(), context.clone());
        let world = Rc::new(RefCell::new(
            host::World::from_snapshot(&json!({"version":1,"nodes":[]}), Some(&view)).unwrap(),
        ));
        let lua = mlua::Lua::new();
        let queue = Rc::new(RefCell::new(scheduler::Scheduler::default()));
        scheduler::install(&lua, &queue).unwrap();
        host::install(&lua, &world).unwrap();
        lua.load("added=0; removed=0; descendants=0; game.ChildAdded:Connect(function(child) added+=1; lastChild=child end); game.ChildRemoved:Connect(function(child) removed+=1 end); game.DescendantAdded:Connect(function(child) descendants+=1 end)").exec().unwrap();
        let before = world.borrow().nodes.materialized_count();
        let create = json!({"version":1,"delta":true,"sequence":1,"actions":[
            {"kind":"create","sequence":1,"node":{"id":"r","class_name":"Part","name":"third","parent":"game","properties":{"Anchored":false}}}
        ]});
        store.apply_delta(&create).unwrap();
        let view = SharedWorldView::projected(store.view(), context.clone());
        host::apply_delta_with_lua(&lua, &world, &create, Some(&view)).unwrap();
        let report = scheduler::step(&lua, &queue, Instant::now(), Duration::from_secs(1));
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(lua.globals().get::<i64>("added").unwrap(), 1);
        assert_eq!(lua.globals().get::<i64>("descendants").unwrap(), 1);
        assert_eq!(
            host::instance_id(&lua.globals().get::<mlua::Value>("lastChild").unwrap()).as_deref(),
            Some("r")
        );
        assert_eq!(
            world.borrow().nodes.materialized_count(),
            before,
            "a received creation must not allocate a mutable wrapper just to emit events"
        );
        let destroy = json!({"version":1,"delta":true,"sequence":2,"actions":[{"kind":"remove","sequence":2,"id":"r"}]});
        store.apply_delta(&destroy).unwrap();
        let view = SharedWorldView::projected(store.view(), context);
        host::apply_delta_with_lua(&lua, &world, &destroy, Some(&view)).unwrap();
        let report = scheduler::step(&lua, &queue, Instant::now(), Duration::from_secs(1));
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(lua.globals().get::<i64>("removed").unwrap(), 1);
        assert_eq!(
            world.borrow().nodes.materialized_count(),
            before,
            "an untouched removed record must stay compact after callbacks"
        );
        // The Lua handle survives removal and must keep its original properties.
        lua.load("assert(lastChild.Parent == nil); assert(lastChild.Name == 'third'); assert(lastChild.Anchored == false); assert(pcall(function() lastChild.Parent = game end))").exec().unwrap();
        assert!(world.borrow().nodes["r"].destroyed);
        // Reparenting works on a destroyed instance, so the Parent override now
        // points at the new container while the node stays destroyed.
        assert_eq!(
            world.borrow().nodes["r"].local_overrides.get("Parent"),
            Some(&json!({"$ref":"game"}))
        );
        // Detach again so the destroyed node returns to its compact form.
        let mut guard = world.borrow_mut();
        let node = guard.nodes.get_mut("r").unwrap();
        node.local_overrides.insert("Parent".into(), Json::Null);
        node.parent = None;
        node.local_revision = 0;
        drop(guard);
        world.borrow_mut().nodes.compact_destroyed("r");
        assert_eq!(world.borrow().nodes.materialized_count(), before);
        let bad = json!({"version":1,"delta":true,"sequence":4,"actions":[
            {"kind":"remove","sequence":3,"id":"q"},{"kind":"unsupported","sequence":4}
        ]});
        assert!(world.borrow_mut().apply_delta(&bad, None).is_err());
        assert!(!world.borrow().nodes["q"].destroyed);
        assert_eq!(world.borrow().nodes["q"].parent.as_deref(), Some("game"));
    }

    #[test]
    fn missing_member_and_named_child_lookup_do_not_materialize_the_world() {
        use crate::rust_host::world as host;
        use crate::{
            engine_world_projection::ProjectionContext, engine_world_view::SharedWorldView,
        };
        use rove_client_core::{schema::Schema, world::WorldStore};
        use std::{cell::RefCell, collections::HashMap, rc::Rc};
        let store = WorldStore::from_snapshot(&json!({"version":1,"nodes":[
            {"id":"game","class_name":"DataModel","name":"game","parent":null},
            {"id":"p","class_name":"Part","name":"same","parent":"game","properties":{}},
            {"id":"q","class_name":"Part","name":"same","parent":"game","properties":{}}
        ]}))
        .unwrap();
        let schema = Schema {
            version: 3,
            flags: 0,
            enums: vec![],
            classes: vec![],
            properties: vec![],
            events: vec![],
            strings: vec![],
            names: vec![],
            lr_types: vec![],
            parent_property_id: 0,
            decoded_bytes: 0,
        };
        let view = SharedWorldView::projected(
            store.view(),
            ProjectionContext::new(&schema, HashMap::new()).unwrap(),
        );
        let world = Rc::new(RefCell::new(
            host::World::from_snapshot(&json!({"version":1,"nodes":[]}), Some(&view)).unwrap(),
        ));
        let lua = mlua::Lua::new();
        host::install(&lua, &world).unwrap();
        let before = world.borrow().nodes.materialized_count();
        let missing: (bool, String) = lua.load("local ok, message = pcall(function() return game.unknownMember end); return ok, tostring(message)").eval().unwrap();
        assert!(!missing.0);
        assert!(missing.1.contains("unknownMember"));
        let found: mlua::Value = lua.load("return game.same").eval().unwrap();
        assert_eq!(
            host::instance_id(&found).as_deref(),
            Some("p"),
            "duplicate child names retain received order"
        );
        assert_eq!(
            world.borrow().nodes.materialized_count(),
            before,
            "a missing member or child name lookup must only inspect shared headers"
        );
        world.borrow_mut().nodes.get_mut("q").unwrap().name = "localName".into();
        let found: mlua::Value = lua.load("return game.localName").eval().unwrap();
        assert_eq!(
            host::instance_id(&found).as_deref(),
            Some("q"),
            "local header edits take precedence over the received name"
        );
    }
}
