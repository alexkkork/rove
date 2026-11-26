use super::world::{
    children, emit, get_property, instance, instance_id, register_method, set_property, World,
    WorldRef,
};
use mlua::{Lua, MultiValue, Value};
use serde_json::{json, Value as Json};
use std::{cell::RefCell, rc::Rc};

fn fixture() -> WorldRef {
    let snapshot = json!({"version":1,"sequence":3,"services":{"Workspace":"ws"},"nodes":[
        {"id":"game","class_name":"DataModel","name":"game","parent":null,"properties":{},"server_owned":true},
        {"id":"ws","class_name":"Workspace","name":"Workspace","parent":"game","properties":{"Gravity":196.2},"server_owned":true},
        {"id":"p","class_name":"Part","name":"Part","parent":"ws","properties":{"Anchored":false},"server_owned":true}
    ]});
    Rc::new(RefCell::new(World::from_snapshot(&snapshot, None).unwrap()))
}

#[test]
fn instance_tostring_uses_current_name_independent_of_parent_identity() {
    let lua = Lua::new();
    let ctx = fixture();
    super::world::install(&lua, &ctx).unwrap();
    lua.load(r#"
        local part = workspace.Part
        assert(tostring(part) == 'Part')
        assert(workspace:GetFullName() == 'Workspace')
        assert(part:GetFullName() == 'Workspace.Part')
        part.Name = 'renamed'
        assert(tostring(part) == 'renamed')
        part.Parent = nil
        assert(tostring(part) == 'renamed')
        assert(part:GetFullName() == 'renamed')
        part:Destroy()
        assert(tostring(part) == 'renamed')
        local humanoid = Instance.new('Humanoid')
        humanoid.Parent = workspace
        local function capture() return humanoid end
        assert(tostring(capture()) == 'Humanoid')
        local folder = Instance.new('Folder')
        folder.Name = 'detached'
        humanoid.Parent = folder
        assert(humanoid:GetFullName() == 'detached.Humanoid')
    "#).exec().unwrap();
}

#[test]
fn player_character_default_is_nil_only_for_complete_received_creates() {
    use crate::{engine_world_projection::ProjectionContext, engine_world_view::SharedWorldView};
    use rove_client_core::{schema::Schema, world::WorldStore};
    use std::collections::HashMap;

    let snapshot = json!({"version":1,"sequence":1,"nodes":[
        {"id":"game","class_name":"DataModel","name":"game","parent":null,"properties":{}},
        {"id":"complete","class_name":"Player","name":"Complete","parent":"game",
            "received_creation_complete":true,"properties":{}},
        {"id":"explicit_nil","class_name":"Player","name":"ExplicitNil","parent":"game",
            "received_creation_complete":true,"properties":{"Character":null}},
        {"id":"explicit_ref","class_name":"Player","name":"ExplicitRef","parent":"game",
            "received_creation_complete":true,"properties":{"Character":{"$ref":"character"}}},
        {"id":"partial","class_name":"Player","name":"Partial","parent":"game",
            "received_creation_complete":false,"properties":{}},
        {"id":"character","class_name":"Model","name":"Character","parent":"game","properties":{}}
    ]});
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
    let projection = ProjectionContext::new(&schema, HashMap::new()).unwrap();
    let store = WorldStore::from_snapshot(&snapshot).unwrap();
    let view = SharedWorldView::projected(store.view(), projection);

    for shared in [false, true] {
        let world = Rc::new(RefCell::new(
            World::from_snapshot(&snapshot, shared.then_some(&view)).unwrap(),
        ));
        assert_eq!(
            get_property(&world, "complete", "Character").unwrap(),
            Json::Null
        );
        assert_eq!(
            get_property(&world, "explicit_nil", "Character").unwrap(),
            Json::Null
        );
        assert_eq!(
            get_property(&world, "explicit_ref", "Character").unwrap(),
            json!({"$ref":"character"})
        );
        assert!(get_property(&world, "partial", "Character").is_err());
    }
}

#[test]
fn received_complete_player_defaults_match_pinned_values_and_explicit_receipts() {
    use crate::{engine_world_projection::ProjectionContext, engine_world_view::SharedWorldView};
    use rove_client_core::{schema::Schema, world::WorldStore};
    use std::collections::HashMap;

    let snapshot = json!({"version":1,"sequence":1,"nodes":[
        {"id":"game","class_name":"DataModel","name":"game","parent":null,"properties":{}},
        {"id":"defaults","class_name":"Player","name":"Defaults","parent":"game",
            "received_creation_complete":true,"properties":{}},
        {"id":"explicit","class_name":"Player","name":"Explicit","parent":"game",
            "received_creation_complete":true,"properties":{
                "CameraMinZoomDistance":3.0,"DevEnableMouseLock":false,
                "CameraMode":{"$enum":{"type":"CameraMode","name":"LockFirstPerson","value":1}}}},
        {"id":"partial","class_name":"Player","name":"Partial","parent":"game",
            "received_creation_complete":false,"properties":{}}
    ]});
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
    let projection = ProjectionContext::new(&schema, HashMap::new()).unwrap();
    let store = WorldStore::from_snapshot(&snapshot).unwrap();
    let view = SharedWorldView::projected(store.view(), projection);
    let defaults = [
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
    ];

    for shared in [false, true] {
        let world = Rc::new(RefCell::new(
            World::from_snapshot(&snapshot, shared.then_some(&view)).unwrap(),
        ));
        for (key, expected) in &defaults {
            assert_eq!(
                get_property(&world, "defaults", key).unwrap(),
                expected.clone(),
                "{key}, shared={shared}"
            );
        }
        assert_eq!(
            get_property(&world, "explicit", "CameraMinZoomDistance").unwrap(),
            json!(3.0)
        );
        assert_eq!(
            get_property(&world, "explicit", "DevEnableMouseLock").unwrap(),
            json!(false)
        );
        assert_eq!(
            get_property(&world, "explicit", "CameraMode").unwrap(),
            json!({"$enum":{"type":"CameraMode","name":"LockFirstPerson","value":1}})
        );
        for (key, _) in &defaults {
            assert!(
                get_property(&world, "partial", key).is_err(),
                "{key}, shared={shared}"
            );
        }
    }
}

#[test]
fn completed_received_models_get_private_initial_scale_one() -> mlua::Result<()> {
    use crate::{engine_world_projection::ProjectionContext, engine_world_view::SharedWorldView};
    use rove_client_core::{schema::Schema, world::WorldStore};
    use std::collections::HashMap;

    let snapshot = json!({"version":1,"sequence":1,"services":{"Workspace":"ws"},"nodes":[
        {"id":"game","class_name":"DataModel","name":"game","parent":null,"properties":{}},
        {"id":"ws","class_name":"Workspace","name":"Workspace","parent":"game","properties":{}},
        {"id":"model","class_name":"Model","name":"Model","parent":"ws",
            "received_creation_complete":true,"properties":{}},
        {"id":"actor","class_name":"Actor","name":"Actor","parent":"ws",
            "received_creation_complete":true,"properties":{}},
        {"id":"explicit","class_name":"Model","name":"Explicit","parent":"ws",
            "received_creation_complete":true,"properties":{"Scale":2.5}},
        {"id":"partial","class_name":"Model","name":"Partial","parent":"ws",
            "received_creation_complete":false,"properties":{}},
        {"id":"part","class_name":"Part","name":"Part","parent":"model",
            "received_creation_complete":true,"properties":{
                "Size":{"$vector3":[2.0,3.0,4.0]},
                "CFrame":{"$cframe":{"position":[1,2,3],"rotation":[1,0,0,0,1,0,0,0,1]}}}}
    ]});
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
    let projection = ProjectionContext::new(&schema, HashMap::new()).unwrap();
    let store = WorldStore::from_snapshot(&snapshot).unwrap();
    let view = SharedWorldView::projected(store.view(), projection);

    for shared in [false, true] {
        let world = Rc::new(RefCell::new(
            World::from_snapshot(&snapshot, shared.then_some(&view)).unwrap(),
        ));
        let before_size = get_property(&world, "part", "Size").unwrap();
        let before_frame = get_property(&world, "part", "CFrame").unwrap();
        let lua = Lua::new();
        super::world::install(&lua, &world)?;
        super::movement::install(&lua, &world)?;
        lua.load(r#"
            assert(workspace.Model:GetScale() == 1)
            assert(workspace.Actor:GetScale() == 1)
            assert(workspace.Explicit:GetScale() == 2.5)
            assert(not pcall(function() return workspace.Model.Scale end))
            assert(not pcall(function() workspace.Model.Scale = 2 end))
            assert(not pcall(function() return workspace.Partial:GetScale() end))
        "#).exec()?;
        assert_eq!(get_property(&world, "part", "Size").unwrap(), before_size);
        assert_eq!(
            get_property(&world, "part", "CFrame").unwrap(),
            before_frame
        );
    }
    Ok(())
}

#[test]
fn received_character_reference_still_fires_character_added() -> mlua::Result<()> {
    use std::time::{Duration, Instant};

    let snapshot = json!({"version":1,"sequence":1,"services":{"Players":"players"},
    "nodes":[
        {"id":"game","class_name":"DataModel","name":"game","parent":null,"properties":{}},
        {"id":"players","class_name":"Players","name":"Players","parent":"game","properties":{}},
        {"id":"player","class_name":"Player","name":"Tester","parent":"players",
            "received_creation_complete":true,"properties":{}}
    ]});
    let world = Rc::new(RefCell::new(World::from_snapshot(&snapshot, None).unwrap()));
    let lua = Lua::new();
    let scheduler = Rc::new(RefCell::new(super::scheduler::Scheduler::new()));
    super::scheduler::install(&lua, &scheduler)?;
    super::signals::install(&lua, &world)?;
    super::world::install(&lua, &world)?;
    lua.load(
        r#"
        local player = game:GetService("Players"):FindFirstChild("Tester")
        added = nil
        player.CharacterAdded:Connect(function(character) added = character end)
    "#,
    )
    .exec()?;

    let delta = json!({"version":1,"delta":true,"sequence":3,"actions":[
        {"sequence":2,"kind":"create","node":{"id":"character","class_name":"Model",
            "name":"Character","parent":"game","received_creation_complete":true,"properties":{}}},
        {"sequence":3,"kind":"set","id":"player","property":"Character","value":{"$ref":"character"}}
    ]});
    super::world::apply_delta_with_lua(&lua, &world, &delta, None)?;
    for _ in 0..3 {
        let report =
            super::scheduler::step(&lua, &scheduler, Instant::now(), Duration::from_millis(20));
        assert!(report.errors.is_empty(), "{:?}", report.errors);
    }
    assert!(lua
        .load("return added == game:FindFirstChild('Character')")
        .eval::<bool>()?);
    Ok(())
}

fn transaction_state(world: &WorldRef) -> Json {
    let order = super::world::ordered_ids(world);
    let world = world.borrow();
    let nodes = world
        .nodes
        .iter()
        .map(|(id, node)| {
            (
                id.clone(),
                json!({
                    "id":node.id, "class":node.class_name, "name":node.name, "parent":node.parent,
                    "destroyed":node.destroyed, "server_owned":node.server_owned,
                    "properties":node.properties, "overrides":node.local_overrides,
                    "attributes":node.attributes, "attributes_known":node.attributes_known,
                    "revisions":node.revisions, "local_revision":node.local_revision,
                    "tags":node.collection_tags, "tags_known":node.collection_tags_known,
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    json!({"nodes":nodes,"order":order,"services":world.services,
        "snapshot":world.snapshot,"sequence":world.sequence,"effects":world.effects})
}

#[test]
fn failed_receipt_restores_repeated_edits_creates_and_reparented_destroy_subtrees() {
    let world = fixture();
    world.borrow_mut().apply_delta(&json!({"version":1,"delta":true,"sequence":5,"actions":[
        {"kind":"create","sequence":4,"node":{"id":"folder","class_name":"Folder","parent":"ws"}},
        {"kind":"create","sequence":5,"node":{"id":"child","class_name":"Part","parent":"p"}}
    ]}), None).unwrap();
    world
        .borrow_mut()
        .nodes
        .get_mut("p")
        .unwrap()
        .local_overrides
        .insert("Name".into(), json!("local name"));
    let before = transaction_state(&world);
    let error = world.borrow_mut().apply_delta(&json!({"version":1,"delta":true,"sequence":14,"actions":[
        {"kind":"set","sequence":6,"id":"p","fields":{"Parent":{"$ref":"folder"},"Name":"renamed"}},
        {"kind":"attribute","sequence":7,"id":"p","attribute_update":{"name":"score","known":true,"value":42}},
        {"kind":"set_tags","sequence":8,"id":"p","tags":["changed"]},
        {"kind":"destroy","sequence":9,"id":"folder"},
        {"kind":"create","sequence":10,"node":{"id":"p","class_name":"Folder","parent":"ws"}},
        {"kind":"create","sequence":11,"node":{"id":"new","class_name":"Folder","parent":"p"}},
        {"kind":"set","sequence":12,"id":"new","property":"Name","value":"changed twice"},
        {"kind":"create","sequence":13,"node":{"id":"new","class_name":"Part","parent":"ws"}},
        {"kind":"unsupported","sequence":14}
    ]}), None).unwrap_err();
    assert!(error.contains("unsupported"));
    assert_eq!(transaction_state(&world), before);
}

#[test]
fn late_service_failure_restores_metadata_service_parent_and_graph() {
    let world = fixture();
    world
        .borrow_mut()
        .apply_delta(
            &json!({"version":1,"delta":true,"sequence":4,"actions":[
        {"kind":"create","sequence":4,"node":{"id":"ws2","class_name":"Workspace","parent":null}}
    ],"chat_enabled":false}),
            None,
        )
        .unwrap();
    let before = transaction_state(&world);
    let error = world
        .borrow_mut()
        .apply_delta(
            &json!({"version":1,"delta":true,"sequence":5,
                "chat_enabled":true,"new_metadata":"must disappear",
                "services":{"Workspace":"ws2","ZMissingService":"missing"},
                "actions":[{"kind":"set","sequence":5,"id":"p","property":"Name","value":"changed"}]
            }),
            None,
        )
        .unwrap_err();
    assert!(error.contains("service reference"));
    assert_eq!(transaction_state(&world), before);
}

#[test]
fn successful_small_receipt_preserves_untouched_node_allocations_and_local_effects() {
    let world = fixture();
    let untouched_name = world.borrow().nodes["ws"].name.as_ptr();
    world
        .borrow_mut()
        .effects
        .insert("pending".into(), json!(["keep"]));
    world
        .borrow_mut()
        .apply_delta(
            &json!({"version":1,"delta":true,"sequence":4,"actions":[
                {"kind":"set","sequence":4,"id":"p","fields":{"Name":"updated","Anchored":true}}
            ]}),
            None,
        )
        .unwrap();
    let state = world.borrow();
    assert_eq!(
        state.nodes["ws"].name.as_ptr(),
        untouched_name,
        "a small receipt must not copy unrelated node payloads"
    );
    assert_eq!(state.nodes["p"].name, "updated");
    assert_eq!(state.nodes["p"].properties["Anchored"], json!(true));
    assert_eq!(state.nodes["p"].revisions["Anchored"], json!(4));
    assert_eq!(state.effects["pending"], json!(["keep"]));
    assert_eq!(state.sequence, 4);
}

#[test]
fn cached_children_remain_ordered_independent_and_fresh_after_mutation_and_fork() {
    let world = fixture();
    let mut first = children(&world, "ws");
    first.clear();
    assert_eq!(children(&world, "ws"), vec!["p"]);
    world.borrow_mut().nodes.get_mut("p").unwrap().parent = Some("game".into());
    assert!(children(&world, "ws").is_empty());
    assert_eq!(children(&world, "game"), vec!["ws", "p"]);
    for node in world.borrow_mut().nodes.values_mut() {
        if node.id == "p" {
            node.parent = Some("ws".into());
        }
    }
    assert_eq!(children(&world, "game"), vec!["ws"]);
    assert_eq!(children(&world, "ws"), vec!["p"]);
    let fork = Rc::new(RefCell::new(world.borrow().fork_for_vm()));
    fork.borrow_mut().nodes.get_mut("p").unwrap().destroyed = true;
    assert!(children(&fork, "ws").is_empty());
    assert_eq!(children(&world, "ws"), vec!["p"]);
    let before = transaction_state(&world);
    assert!(world
        .borrow_mut()
        .apply_delta(
            &json!({"version":1,"delta":true,"sequence":5,"actions":[
                {"kind":"destroy","sequence":4,"id":"p"},
                {"kind":"set","sequence":5,"id":"missing","property":"Name","value":"invalid"}
            ]}),
            None
        )
        .is_err());
    assert_eq!(transaction_state(&world), before);
    assert_eq!(children(&world, "ws"), vec!["p"]);
}

#[test]
fn authored_create_reparent_and_destroy_invalidate_sibling_cache() {
    let world = fixture();
    let lua = Lua::new();
    super::signals::install(&lua, &world).unwrap();
    super::world::install(&lua, &world).unwrap();
    lua.load(
        r#"
        assert(#workspace:GetChildren() == 1)
        local folder = Instance.new('Folder', workspace)
        local children = workspace:GetChildren()
        assert(#children == 2 and children[1].Name == 'Part' and children[2] == folder)
        folder.Parent = game
        assert(#workspace:GetChildren() == 1)
        folder.Parent = workspace
        assert(#workspace:GetChildren() == 2)
        folder:Destroy()
        assert(#workspace:GetChildren() == 1)
    "#,
    )
    .exec()
    .unwrap();
}

#[test]
fn local_constructors_and_service_bootstrap_preserve_existing_snapshot_ids() {
    let snapshot = json!({"version":1,"nodes":[
        {"id":"game","class_name":"DataModel","name":"game","parent":null,"properties":{}},
        {"id":"local:1","class_name":"Folder","name":"KeepMe","parent":"game",
         "server_owned":false,"properties":{"Archivable":true}}
    ]});
    let world = Rc::new(RefCell::new(World::from_snapshot(&snapshot, None).unwrap()));
    let lua = Lua::new();
    super::world::install(&lua, &world).unwrap();
    super::services::install(&lua, &world).unwrap();
    lua.load(
        r#"
        local original = game:FindFirstChild('KeepMe')
        local created = Instance.new('Folder', game)
        local cloned = original:Clone()
        local settings = UserSettings()
        assert(original.Name == 'KeepMe' and original.Parent == game)
        assert(original ~= created and original ~= cloned)
        assert(settings:IsA('UserSettings'))
    "#,
    )
    .exec()
    .unwrap();
    assert_eq!(world.borrow().nodes["local:1"].class_name, "Folder");
    assert_eq!(world.borrow().nodes["local:1"].name, "KeepMe");
}

#[test]
fn registered_service_roots_are_parented_to_game_on_snapshot_and_delta() {
    let snapshot = json!({"version":1,"sequence":3,"services":{"Workspace":"ws"},"nodes":[
        {"id":"game","class_name":"DataModel","name":"game","parent":null,"properties":{}},
        {"id":"ws","class_name":"Workspace","name":"Workspace","parent":null,"properties":{}},
        {"id":"ws2","class_name":"Workspace","name":"Workspace2","parent":null,"properties":{}}
    ]});
    let mut world = World::from_snapshot(&snapshot, None).unwrap();
    assert_eq!(world.nodes["ws"].parent.as_deref(), Some("game"));

    world
        .apply_delta(
            &json!({"version":1,"delta":true,"sequence":4,"actions":[],"services":{"Workspace":"ws2"}}),
            None,
        )
        .unwrap();
    assert_eq!(world.services["Workspace"], "ws2");
    assert_eq!(world.nodes["ws2"].parent.as_deref(), Some("game"));
}

#[test]
fn reads_received_property_and_reports_missing_without_fabricating_values() {
    let world = fixture();
    assert_eq!(get_property(&world, "p", "Anchored").unwrap(), json!(false));
    assert!(get_property(&world, "p", "Color")
        .unwrap_err()
        .to_string()
        .contains("unavailable in snapshot"));
}

#[test]
fn writes_are_local_overrides_with_expected_mutation_shape() {
    let world = fixture();
    let lua = Lua::new();
    super::signals::install(&lua, &world).unwrap();
    set_property(&lua, &world, "p", "Anchored", json!(true)).unwrap();
    assert_eq!(get_property(&world, "p", "Anchored").unwrap(), json!(true));
    assert_eq!(
        world.borrow().effects["mutations"][0],
        json!({"kind":"set","id":"p","property":"Anchored","value":true})
    );
    assert!(world.borrow().nodes["p"].server_owned);
}

#[test]
fn instance_handles_are_identity_cached_and_hierarchy_is_indexed() {
    let lua = Lua::new();
    let world = fixture();
    let first = instance(&lua, &world, "p").unwrap();
    let second = instance(&lua, &world, "p").unwrap();
    assert_eq!(instance_id(&first), Some("p".into()));
    assert_eq!(instance_id(&second), Some("p".into()));
    let Value::UserData(a) = first else {
        panic!("expected instance userdata")
    };
    let Value::UserData(b) = second else {
        panic!("expected instance userdata")
    };
    assert!(a.to_pointer() == b.to_pointer());
    assert_eq!(children(&world, "ws"), vec!["p"]);
}

#[test]
fn attribute_knowledge_and_deltas_preserve_server_record_ownership() {
    let snapshot = json!({"version":1,"sequence":4,"nodes":[
        {"id":"x","class_name":"Folder","name":"x","parent":null,"attributes_known":true,"attributes":{"tag":"ok"},"properties":{},"server_owned":true}
    ]});
    let mut world = World::from_snapshot(&snapshot, None).unwrap();
    world
        .apply_delta(
            &json!({"version":1,"delta":true,"sequence":5,"actions":[
                {"kind":"set","sequence":5,"id":"x","property":"Name","value":"renamed"}
            ]}),
            None,
        )
        .unwrap();
    assert_eq!(world.nodes["x"].name, "renamed");
    assert!(world.nodes["x"].server_owned);
    assert_eq!(world.nodes["x"].attributes["tag"], json!("ok"));
}

#[test]
fn effect_emitter_appends_without_replacing_prior_entries() {
    let world = fixture();
    emit(&world, "events", json!({"kind":"one"}));
    emit(&world, "events", json!({"kind":"two"}));
    assert_eq!(
        world.borrow().effects["events"].as_array().unwrap().len(),
        2
    );
}

#[test]
fn root_globals_follow_replacement_removal_and_restore_after_property_updates() {
    let world = fixture();
    let lua = Lua::new();
    super::world::install(&lua, &world).unwrap();
    lua.load("original=game; old_workspace=workspace; game=nil; workspace=nil")
        .exec()
        .unwrap();
    super::world::apply_delta_with_lua(
        &lua,
        &world,
        &json!({"version":1,"delta":true,"sequence":4,
        "actions":[{"kind":"set","sequence":4,"id":"p","fields":{"Anchored":true}}]}),
        None,
    )
    .unwrap();
    assert!(lua
        .load("return game==original and workspace==old_workspace")
        .eval::<bool>()
        .unwrap());
    // A repeated ID can replace the root without changing the world size.
    super::world::apply_delta_with_lua(&lua,&world,&json!({"version":1,"delta":true,"sequence":5,
        "actions":[{"kind":"create","sequence":5,"node":{"id":"game","class_name":"Folder","name":"replaced"}}]}),None).unwrap();
    assert!(lua.load("return game==nil").eval::<bool>().unwrap());
    super::world::apply_delta_with_lua(&lua,&world,&json!({"version":1,"delta":true,"sequence":6,
        "actions":[{"kind":"create","sequence":6,"node":{"id":"game","class_name":"DataModel","name":"returned"}}]}),None).unwrap();
    assert_eq!(
        lua.load("return game.Name").eval::<String>().unwrap(),
        "returned"
    );
    // Mutations through the raw world API invalidate the cached sequence too.
    world
        .borrow_mut()
        .apply_delta(
            &json!({"version":1,"delta":true,"sequence":7,
        "actions":[{"kind":"create","sequence":7,"node":{"id":"game","class_name":"Folder"}}]}),
            None,
        )
        .unwrap();
    super::world::apply_delta_with_lua(
        &lua,
        &world,
        &json!({"version":1,"delta":true,"sequence":8,
        "actions":[{"kind":"set","sequence":8,"id":"p","fields":{"Anchored":false}}]}),
        None,
    )
    .unwrap();
    assert!(lua.load("return game==nil").eval::<bool>().unwrap());
}

#[test]
fn loaded_is_fired_on_the_same_preferred_datamodel_exposed_as_game() {
    use std::time::{Duration, Instant};
    let world = Rc::new(RefCell::new(
        World::from_snapshot(
            &json!({"version":1,"sequence":0,"nodes":[
                {"id":"__world_snapshot_datamodel__","class_name":"DataModel","name":"fallback"}
            ]}),
            None,
        )
        .unwrap(),
    ));
    world.borrow_mut().apply_delta(&json!({"version":1,"delta":true,"sequence":1,"actions":[
        {"kind":"create","sequence":1,"node":{"id":"real","class_name":"DataModel","name":"received"}}
    ]}),None).unwrap();
    let lua = Lua::new();
    let sched = Rc::new(RefCell::new(super::scheduler::Scheduler::new()));
    super::scheduler::install(&lua, &sched).unwrap();
    super::world::install(&lua, &world).unwrap();
    lua.load(
        "assert(game.Name=='received'); loaded=0; game.Loaded:Connect(function() loaded+=1 end)",
    )
    .exec()
    .unwrap();
    super::world::apply_delta_with_lua(
        &lua,
        &world,
        &json!({"version":1,"delta":true,"sequence":2,
        "initial_replication_complete":true,"actions":[]}),
        None,
    )
    .unwrap();
    let step = super::scheduler::step(&lua, &sched, Instant::now(), Duration::from_millis(100));
    assert!(step.errors.is_empty(), "{:?}", step.errors);
    assert_eq!(lua.globals().get::<i64>("loaded").unwrap(), 1);
    super::world::apply_delta_with_lua(
        &lua,
        &world,
        &json!({"version":1,"delta":true,"sequence":3,
        "actions":[{"kind":"destroy","sequence":3,"id":"real"}]}),
        None,
    )
    .unwrap();
    assert_eq!(
        lua.load("return game.Name").eval::<String>().unwrap(),
        "fallback"
    );
}

#[test]
fn registered_methods_strip_only_the_receiver() {
    fn echo(_lua: &Lua, _ctx: &WorldRef, _id: &str, args: MultiValue) -> mlua::Result<MultiValue> {
        Ok(args)
    }

    let lua = Lua::new();
    let world = fixture();
    register_method(&world, "Part", "Echo", echo);
    super::world::install(&lua, &world).unwrap();
    lua.globals()
        .set("part", instance(&lua, &world, "p").unwrap())
        .unwrap();

    let returned: Value = lua.load("return part:Echo(part)").eval().unwrap();
    assert!(matches!(returned, Value::UserData(_)));

    let error = lua
        .load("return part.Echo('payload')")
        .eval::<Value>()
        .unwrap_err();
    assert!(error.to_string().contains("receiver"));
}

#[test]
fn actor_world_sync_preserves_actor_local_nodes_and_overrides() {
    let root = fixture();
    let actor = Rc::new(RefCell::new(root.borrow().fork_for_actor("abc")));
    let lua = Lua::new();
    super::signals::install(&lua, &actor).unwrap();
    super::world::install(&lua, &actor).unwrap();
    let local = super::world::create_instance(&lua, &actor, "Folder", Some("ws")).unwrap();
    let actor_id = instance_id(&local).unwrap();
    assert!(actor_id.starts_with("local:actor:616263:"));
    actor
        .borrow_mut()
        .nodes
        .get_mut("p")
        .unwrap()
        .local_overrides
        .insert("Anchored".into(), json!(true));

    {
        let mut root = root.borrow_mut();
        root.nodes
            .get_mut("p")
            .unwrap()
            .revisions
            .insert("Anchored".into(), 8);
        root.sequence = 8;
    }
    actor.borrow_mut().sync_actor_fork(&root.borrow());

    assert!(actor.borrow().nodes.contains_key(&actor_id));
    assert_eq!(
        actor.borrow().nodes["p"].local_overrides["Anchored"],
        json!(true)
    );
    assert_eq!(actor.borrow().nodes["p"].revisions["Anchored"], json!(8));
    assert_eq!(actor.borrow().sequence, 8);
    assert_eq!(actor.borrow().nodes[&actor_id].class_name, "Folder");
    assert_eq!(actor.borrow().nodes[&actor_id].name, "Folder");
    assert_eq!(
        actor.borrow().nodes[&actor_id].parent.as_deref(),
        Some("ws")
    );
    assert!(super::world::ordered_ids(&actor).contains(&actor_id));
}

#[test]
fn actor_sync_keeps_untouched_received_records_lazy() {
    use crate::{engine_world_projection::ProjectionContext, engine_world_view::SharedWorldView};
    use rove_client_core::{schema::Schema, world::WorldStore};
    use std::collections::HashMap;
    let mut nodes = vec![json!({"id":"ws","class_name":"Workspace","name":"Workspace"})];
    for i in 0..1000 {
        nodes.push(json!({"id":format!("p{i}"),"class_name":"Part","name":"Part","parent":"ws","properties":{"Anchored":false}}));
    }
    let store =
        WorldStore::from_snapshot(&json!({"version":1,"sequence":0,"nodes":nodes})).unwrap();
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
    let root =
        World::from_snapshot(&json!({"version":1,"sequence":0,"nodes":[]}), Some(&view)).unwrap();
    let mut actor = root.fork_for_actor("worker");
    // Runtime setup may materialize service headers; none of the parts should
    // need a wrapper until a script actually reads or writes one.
    let baseline = root.nodes.materialized_count();
    for i in 0..1000 {
        assert!(root.nodes.peek(&format!("p{i}")).is_none());
    }
    actor.sync_actor_fork(&root);
    assert_eq!(actor.nodes.materialized_count(), baseline);
    assert!(actor.nodes.peek("p999").is_none());
    assert_eq!(actor.nodes.materialized_count(), baseline);
    actor
        .nodes
        .get_mut("p0")
        .unwrap()
        .local_overrides
        .insert("Anchored".into(), json!(true));
    actor.sync_actor_fork(&root);
    assert_eq!(actor.nodes.materialized_count(), baseline + 1);
    assert_eq!(actor.nodes["p0"].local_overrides["Anchored"], json!(true));
    assert_eq!(root.nodes.materialized_count(), baseline);
}

#[test]
fn instance_query_descendants_matches_snapshot_classes_names_properties_and_relations() {
    let snapshot = json!({"version":1,"sequence":1,"services":{"Workspace":"ws"},"nodes":[
        {"id":"game","class_name":"DataModel","name":"game","parent":null,"properties":{}},
        {"id":"ws","class_name":"Workspace","name":"Workspace","parent":"game","properties":{}},
        {"id":"actor","class_name":"Actor","name":"Actor","parent":"ws","properties":{}},
        {"id":"part","class_name":"Part","name":"Probe","parent":"actor","properties":{"Anchored":false},"collection_tags":["probe"]},
        {"id":"other","class_name":"Part","name":"Other","parent":"ws","properties":{"Anchored":true}}
    ]});
    let world = Rc::new(RefCell::new(World::from_snapshot(&snapshot, None).unwrap()));
    let lua = Lua::new();
    super::world::install(&lua, &world).unwrap();
    let result: bool = lua
        .load(
            r#"
        local direct = workspace:QueryDescendants("> Actor > Part#Probe[Anchored=false]")
        assert(#direct == 1 and direct[1].Name == "Probe")
        local has = workspace:QueryDescendants("Actor:has(Part)")
        assert(#has == 1 and has[1].ClassName == "Actor")
        local not_other = workspace:QueryDescendants("Part:not(#Other)")
        assert(#not_other == 1 and not_other[1].Name == "Probe")
        return true
    "#,
        )
        .eval()
        .unwrap();
    assert!(result);
}

#[test]
fn thread_identity_capabilities_gate_data_cost_player_creation_and_player_writes() {
    use std::time::{Duration, Instant};
    let snapshot = json!({"version":1,"sequence":0,"nodes":[
        {"id":"game","class_name":"DataModel","name":"game","properties":{}}
    ]});
    let world = Rc::new(RefCell::new(World::from_snapshot(&snapshot, None).unwrap()));
    let lua = Lua::new();
    let scheduler = Rc::new(RefCell::new(super::scheduler::Scheduler::new()));
    super::scheduler::install(&lua, &scheduler).unwrap();
    super::world::install(&lua, &world).unwrap();
    let script = lua.load(r#"
        setthreadidentity(0)
        local ok, message = pcall(function() return game.DataCost end)
        assert(not ok and string.find(tostring(message), 'lacking capability LocalUser', 1, true))

        setthreadidentity(6)
        ok, message = pcall(Instance.new, 'Player')
        assert(not ok and string.find(tostring(message), 'lacking capability WritePlayer', 1, true))

        setthreadidentity(8)
        local player = Instance.new('Player')
        assert(player:IsA('Player') and player.ClassName == 'Player')
        assert(player.UserId == 0 and player.GameplayPaused == false and player.Parent == nil)

        setthreadidentity(2)
        ok, message = pcall(function() player.GameplayPaused = true end)
        assert(not ok and string.find(tostring(message), 'lacking capability RobloxScript', 1, true))
        assert(player.GameplayPaused == false)

        setthreadidentity(8)
        task.spawn(function()
            child_identity = getthreadidentity()
            child_player_class = Instance.new('Player').ClassName
        end)
    "#).into_function().unwrap();
    let root = super::scheduler::enqueue_with_policy(
        &lua,
        script,
        MultiValue::new(),
        super::scheduler::TaskPolicy::default(),
    )
    .unwrap();
    for _ in 0..8 {
        let report =
            super::scheduler::step(&lua, &scheduler, Instant::now(), Duration::from_millis(20));
        assert!(report.errors.is_empty(), "{:?}", report.errors);
    }
    assert!(matches!(
        super::scheduler::take_outcome(&scheduler, &root),
        Some(super::scheduler::TaskOutcome::Completed(_))
    ));
    assert_eq!(lua.globals().get::<i64>("child_identity").unwrap(), 8);
    assert_eq!(
        lua.globals().get::<String>("child_player_class").unwrap(),
        "Player"
    );
}

#[test]
fn property_default_methods_reset_only_known_writable_values_and_get_actor_walks_ancestors() {
    let snapshot = json!({"version":1,"sequence":1,"services":{"Workspace":"ws"},"nodes":[
        {"id":"game","class_name":"DataModel","name":"game","parent":null,"properties":{}},
        {"id":"ws","class_name":"Workspace","name":"Workspace","parent":"game","properties":{}},
        {"id":"actor","class_name":"Actor","name":"Actor","parent":"ws","properties":{}},
        {"id":"part","class_name":"Part","name":"Part","parent":"actor","properties":{"Anchored":false}}
    ]});
    let world = Rc::new(RefCell::new(World::from_snapshot(&snapshot, None).unwrap()));
    let lua = Lua::new();
    super::signals::install(&lua, &world).unwrap();
    super::world::install(&lua, &world).unwrap();
    let result: bool = lua
        .load(
            r#"
        local p=workspace:FindFirstChild("Actor"):FindFirstChild("Part")
        assert(p:GetActor().Name=="Actor")
        assert(not p:IsPropertyModified("Anchored"))
        p.Anchored=true
        assert(p:IsPropertyModified("Anchored"))
        p:ResetPropertyToDefault("Anchored")
        assert(p.Anchored==false and not p:IsPropertyModified("Anchored"))
        p.Name="Changed"
        assert(p:IsPropertyModified("Name"))
        p:ResetPropertyToDefault("Name")
        assert(p.Name=="Part" and not p:IsPropertyModified("Name"))
        return true
    "#,
        )
        .eval()
        .unwrap();
    assert!(result);
}

#[test]
fn client_property_origin_reports_received_local_and_unknown_snapshot_state() {
    let world = fixture();
    let lua = Lua::new();
    super::signals::install(&lua, &world).unwrap();
    super::world::install(&lua, &world).unwrap();
    lua.globals()
        .set("part", instance(&lua, &world, "p").unwrap())
        .unwrap();
    let origins: (String, String, String) = lua
        .load(
            r#"
        local received=client.propertyOrigin(part,"Anchored")
        part.Anchored=true
        return received,client.propertyOrigin(part,"Anchored"),client.propertyOrigin(part,"Color")
    "#,
        )
        .eval()
        .unwrap();
    assert_eq!(
        origins,
        ("received".into(), "local".into(), "unavailable".into())
    );
}

#[test]
fn engine_global_singletons_are_stable_local_nodes_without_create_mutations() {
    let world = fixture();
    let lua = Lua::new();
    super::world::install(&lua, &world).unwrap();
    let mutation_count = world
        .borrow()
        .effects
        .get("mutations")
        .and_then(Json::as_array)
        .map_or(0, Vec::len);
    let settings = super::world::ensure_engine_global_node(&lua, &world, "GlobalSettings").unwrap();
    let settings_id = instance_id(&settings).unwrap();
    let again = super::world::ensure_engine_global_node(&lua, &world, "GlobalSettings").unwrap();
    assert_eq!(instance_id(&again).as_deref(), Some(settings_id.as_str()));
    let plugin = super::world::ensure_engine_global_node(&lua, &world, "PluginManager").unwrap();
    lua.globals().set("settings", settings).unwrap();
    lua.globals().set("plugin", plugin).unwrap();
    let ancestry: bool = lua.load("return settings:IsA('GenericSettings') and settings:IsA('ServiceProvider') and plugin:IsA('Instance')").eval().unwrap();
    assert!(ancestry);
    assert_eq!(
        world
            .borrow()
            .effects
            .get("mutations")
            .and_then(Json::as_array)
            .map_or(0, Vec::len),
        mutation_count
    );
    assert!(!world.borrow().services.contains_key("PluginManager"));

    world
        .borrow_mut()
        .nodes
        .get_mut(&settings_id)
        .unwrap()
        .destroyed = true;
    let recreated =
        super::world::ensure_engine_global_node(&lua, &world, "GlobalSettings").unwrap();
    assert_eq!(
        instance_id(&recreated).as_deref(),
        Some(settings_id.as_str())
    );
    assert!(!world.borrow().nodes[&settings_id].destroyed);
    assert_eq!(
        world
            .borrow()
            .effects
            .get("mutations")
            .and_then(Json::as_array)
            .map_or(0, Vec::len),
        mutation_count
    );
}

#[test]
fn instance_handle_constructor_null_get_identity_and_lazy_service_lookup() {
    let world = fixture();
    let lua = Lua::new();
    super::world::install(&lua, &world).unwrap();
    lua.globals()
        .set("part", instance(&lua, &world, "p").unwrap())
        .unwrap();
    let encoded_null = super::values::to_json(
        &lua,
        &super::world::value_from_json(&lua, &world, &json!({"$instance_handle":null})).unwrap(),
    )
    .unwrap();
    assert_eq!(encoded_null, json!({"$instance_handle":null}));
    let result: bool = lua
        .load(
            r#"
        local first=InstanceHandle.new(part)
        local second=InstanceHandle.new(part)
        assert(first==second and first:Get()==part)
        local null=InstanceHandle.new()
        assert(null:Get()==nil and tostring(null)=="nil")
        assert(null==InstanceHandle.new(nil))
        assert(first.Get(null)==nil and null.Get(first)==part)
        assert(first.Wait(null,0.01)==nil and null.Wait(first,0.01)==part)
        assert(not pcall(function() first:Wait(0) end))
        assert(not pcall(function() null:Wait(-1) end))
        assert(game:GetService("UserInputService")==game:FindService("UserInputService"))
        assert(game:service("CoreGui")==game:GetService("CoreGui"))
        local panel=Instance.new("ScreenGui")
        panel.Parent=game:service("CoreGui")
        assert(panel.Parent==game:GetService("CoreGui"))
        panel:Destroy()
        assert(game:FindService("PathfindingService")==nil)
        local path=game:GetService("PathfindingService")
        assert(path==game:GetService("PathfindingService"))
        assert(not pcall(function() workspace:GetService("RunService") end))
        assert(not pcall(function() workspace:service("RunService") end))
        assert(not pcall(function() game:service("NotARealService") end))
        assert(not pcall(function() game:GetService("NotARealService") end))
        return true
    "#,
        )
        .eval()
        .unwrap();
    assert!(result);
}

#[test]
fn instance_handle_wait_resolves_after_received_node_delta() {
    use std::time::{Duration, Instant};
    let world = fixture();
    let lua = Lua::new();
    let scheduler = Rc::new(RefCell::new(super::scheduler::Scheduler::default()));
    super::scheduler::install(&lua, &scheduler).unwrap();
    super::world::install(&lua, &world).unwrap();
    let handle =
        super::world::value_from_json(&lua, &world, &json!({"$instance_handle":"future-node"}))
            .unwrap();
    let function: mlua::Function = lua
        .load("return function(handle) local node=handle:Wait(); return node and node.Name end")
        .eval()
        .unwrap();
    let thread = super::scheduler::enqueue_with_policy(
        &lua,
        function,
        mlua::MultiValue::from_vec(vec![handle]),
        super::scheduler::TaskPolicy::default(),
    )
    .unwrap();
    let report = super::scheduler::step(&lua, &scheduler, Instant::now(), Duration::from_secs(1));
    assert!(
        report.errors.is_empty(),
        "initial Wait step errors: {:?}",
        report.errors
    );
    assert!(super::scheduler::is_parked(&scheduler, &thread));

    world.borrow_mut().apply_delta(&json!({"version":1,"delta":true,"sequence":4,"actions":[{
        "sequence":4,"kind":"create","node":{"id":"future-node","class_name":"Folder","name":"Arrived","parent":"ws","properties":{}}
    }]}), None).unwrap();
    super::world::resolve_instance_handle_waiters(&lua, &world).unwrap();
    let report = super::scheduler::step(&lua, &scheduler, Instant::now(), Duration::from_secs(1));
    assert!(
        report.errors.is_empty(),
        "resolved Wait step errors: {:?}",
        report.errors
    );
    match super::scheduler::take_outcome(&scheduler, &thread).unwrap() {
        super::scheduler::TaskOutcome::Completed(values) => assert_eq!(
            values.front(),
            Some(&Value::String(lua.create_string("Arrived").unwrap()))
        ),
        other => panic!("expected resolved handle result, got {other:?}"),
    }
}

#[test]
fn instance_handle_wait_is_scoped_to_its_world() {
    use std::time::{Duration, Instant};
    let first_world = fixture();
    let second_world = fixture();
    let lua = Lua::new();
    let scheduler = Rc::new(RefCell::new(super::scheduler::Scheduler::default()));
    super::scheduler::install(&lua, &scheduler).unwrap();
    super::world::install(&lua, &first_world).unwrap();
    let handle =
        super::world::value_from_json(&lua, &first_world, &json!({"$instance_handle":"same-id"}))
            .unwrap();
    let function: mlua::Function = lua
        .load("return function(handle) return handle:Wait() end")
        .eval()
        .unwrap();
    let thread = super::scheduler::enqueue_with_policy(
        &lua,
        function,
        MultiValue::from_vec(vec![handle]),
        super::scheduler::TaskPolicy::default(),
    )
    .unwrap();
    let report = super::scheduler::step(&lua, &scheduler, Instant::now(), Duration::from_secs(1));
    assert!(
        report.errors.is_empty(),
        "initial Wait step errors: {:?}",
        report.errors
    );
    assert!(super::scheduler::is_parked(&scheduler, &thread));

    second_world.borrow_mut().apply_delta(&json!({"version":1,"delta":true,"sequence":4,"actions":[{
        "sequence":4,"kind":"create","node":{"id":"same-id","class_name":"Folder","name":"Wrong world","parent":"ws","properties":{}}
    }]}), None).unwrap();
    super::world::resolve_instance_handle_waiters(&lua, &second_world).unwrap();
    assert!(super::scheduler::is_parked(&scheduler, &thread));

    first_world.borrow_mut().apply_delta(&json!({"version":1,"delta":true,"sequence":4,"actions":[{
        "sequence":4,"kind":"create","node":{"id":"same-id","class_name":"Folder","name":"Right world","parent":"ws","properties":{}}
    }]}), None).unwrap();
    super::world::resolve_instance_handle_waiters(&lua, &first_world).unwrap();
    let report = super::scheduler::step(&lua, &scheduler, Instant::now(), Duration::from_secs(1));
    assert!(
        report.errors.is_empty(),
        "resolved Wait step errors: {:?}",
        report.errors
    );
    match super::scheduler::take_outcome(&scheduler, &thread).unwrap() {
        super::scheduler::TaskOutcome::Completed(values) => {
            let Value::UserData(node) = values.front().unwrap() else {
                panic!("expected resolved instance")
            };
            assert_eq!(
                instance_id(&Value::UserData(node.clone())).as_deref(),
                Some("same-id")
            );
            assert_eq!(first_world.borrow().nodes["same-id"].name, "Right world");
        }
        other => panic!("expected resolved handle result, got {other:?}"),
    }
}

#[test]
fn user_settings_service_provider_returns_user_game_settings() {
    let world = fixture();
    let lua = Lua::new();
    super::world::install(&lua, &world).unwrap();
    super::services::install(&lua, &world).unwrap();
    let result: bool = lua
        .load(
            r#"
        local settings=UserSettings()
        local first=settings:GetService("UserGameSettings")
        local second=settings:FindService("UserGameSettings")
        assert(first==second and first.ClassName=="UserGameSettings")
        assert(settings:FindService("MissingSettingsService")==nil)
        assert(not pcall(function() settings:GetService("MissingSettingsService") end))
        return true
    "#,
        )
        .eval()
        .unwrap();
    assert!(result);
}

#[test]
fn clear_all_children_destroys_subtrees_but_keeps_the_container_usable() {
    use std::time::{Duration, Instant};
    let world = fixture();
    let lua = Lua::new();
    let sched = Rc::new(RefCell::new(super::scheduler::Scheduler::new()));
    super::scheduler::install(&lua, &sched).unwrap();
    super::signals::install(&lua, &world).unwrap();
    super::world::install(&lua, &world).unwrap();
    lua.load(r#"
        container=Instance.new('Folder',workspace)
        first=Instance.new('Folder',container); first.Name='first'
        nested=Instance.new('Part',first); nested.Name='nested'
        second=Instance.new('Part',container); second.Name='second'
        destroyed=0; removed={}; added=0
        container.Destroying:Connect(function() error('container destroyed') end)
        keep=container.ChildAdded:Connect(function() added+=1 end)
        workspace.DescendantRemoving:Connect(function(item) table.insert(removed,item.Name) end)
        for _,item in ipairs({first,nested,second}) do item.Destroying:Connect(function() destroyed+=1 end) end
        container:ClearAllChildren()
    "#).exec().unwrap();
    let report = super::scheduler::step(&lua, &sched, Instant::now(), Duration::from_millis(100));
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    lua.load("assert(destroyed==3 and table.concat(removed,',')=='first,nested,second'); assert(container.Parent==workspace and #container:GetChildren()==0 and keep.Connected); for _,item in ipairs({first,nested,second}) do assert(item.Parent==nil and pcall(function() item.Parent=container end)) end; container:ClearAllChildren(); fresh=Instance.new('Part',container)").exec().unwrap();
    assert!(
        super::scheduler::step(&lua, &sched, Instant::now(), Duration::from_millis(100))
            .errors
            .is_empty()
    );
    lua.load("assert(added==1 and #container:GetChildren()==1)")
        .exec()
        .unwrap();
}

#[test]
fn cancelled_child_waits_release_capacity_and_never_resume_when_the_child_arrives() {
    use std::time::{Duration, Instant};
    let world = fixture();
    let lua = Lua::new();
    let sched = Rc::new(RefCell::new(super::scheduler::Scheduler::new()));
    super::scheduler::install(&lua, &sched).unwrap();
    super::world::install(&lua, &world).unwrap();
    lua.load("resumed=0; function start() thread=task.spawn(function() workspace:WaitForChild('future'); resumed+=1 end) end; function stop() task.cancel(thread) end").exec().unwrap();
    let start: mlua::Function = lua.globals().get("start").unwrap();
    let stop: mlua::Function = lua.globals().get("stop").unwrap();
    for _ in 0..2050 {
        start.call::<()>(()).unwrap();
        let report =
            super::scheduler::step(&lua, &sched, Instant::now(), Duration::from_millis(100));
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        stop.call::<()>(()).unwrap();
    }
    start.call::<()>(()).unwrap();
    super::scheduler::step(&lua, &sched, Instant::now(), Duration::from_millis(100));
    super::world::apply_delta_with_lua(&lua,&world,&json!({"version":1,"delta":true,"sequence":4,"actions":[
        {"kind":"create","sequence":4,"node":{"id":"future","class_name":"Folder","name":"future","parent":"ws"}}
    ]}),None).unwrap();
    let report = super::scheduler::step(&lua, &sched, Instant::now(), Duration::from_millis(100));
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(lua.globals().get::<i64>("resumed").unwrap(), 1);
}

#[test]
fn wait_for_child_case_aliases_work_through_namecall_and_indexed_calls() {
    use std::time::{Duration, Instant};

    let world = fixture();
    let lua = Lua::new();
    let sched = Rc::new(RefCell::new(super::scheduler::Scheduler::new()));
    super::scheduler::install(&lua, &sched).unwrap();
    super::signals::install(&lua, &world).unwrap();
    super::closures::install(&lua).unwrap();
    super::world::install(&lua, &world).unwrap();

    lua.load(r#"
        local previous
        previous = hookmetamethod(workspace, "__namecall", function(self, ...)
            if getnamecallmethod() == "waitForChild" then namecall_alias_seen = true end
            return previous(self, ...)
        end)
        task.spawn(function()
            namecall_child = workspace:waitForChild("future-namecall")
        end)
        task.spawn(function()
            local wait_for_child = workspace.waitForChild
            indexed_child = wait_for_child(workspace, "future-indexed")
        end)
        task.spawn(function()
            uppercase_child = workspace:WaitForChild("future-uppercase")
        end)
    "#).exec().unwrap();

    let waiting = super::scheduler::step(&lua, &sched, Instant::now(), Duration::from_millis(100));
    assert!(waiting.errors.is_empty(), "{:?}", waiting.errors);
    assert_eq!(super::scheduler::snapshot(&sched).waiting, 3);

    lua.load(r#"
        for _, name in ipairs({"future-namecall", "future-indexed", "future-uppercase"}) do
            local child = Instance.new("Folder", workspace)
            child.Name = name
        end
    "#).exec().unwrap();
    let resumed = super::scheduler::step(&lua, &sched, Instant::now(), Duration::from_millis(100));
    assert!(resumed.errors.is_empty(), "{:?}", resumed.errors);
    let result: bool = lua.load(r#"
        return namecall_alias_seen
            and namecall_child.Name == "future-namecall"
            and indexed_child.Name == "future-indexed"
            and uppercase_child.Name == "future-uppercase"
    "#).eval().unwrap();
    assert!(result);
}

#[test]
fn cancelled_instance_handle_timeout_cannot_restart_its_coroutine() {
    use std::time::{Duration, Instant};
    let world = fixture();
    let lua = Lua::new();
    let sched = Rc::new(RefCell::new(super::scheduler::Scheduler::new()));
    super::scheduler::install(&lua, &sched).unwrap();
    super::world::install(&lua, &world).unwrap();
    let handle =
        super::world::value_from_json(&lua, &world, &json!({"$instance_handle":"future"})).unwrap();
    lua.globals().set("handle", handle).unwrap();
    lua.load("resumed=false; thread=task.spawn(function() handle:Wait(0.01); resumed=true end)")
        .exec()
        .unwrap();
    super::scheduler::step(&lua, &sched, Instant::now(), Duration::from_millis(100));
    lua.load("task.cancel(thread)").exec().unwrap();
    std::thread::sleep(Duration::from_millis(20));
    let report = super::scheduler::step(&lua, &sched, Instant::now(), Duration::from_millis(100));
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert!(!lua.globals().get::<bool>("resumed").unwrap());
}

#[test]
fn frame_releases_cancelled_child_wait_timer_without_world_changes() {
    use std::time::{Duration, Instant};
    let world = fixture();
    let lua = Lua::new();
    let sched = Rc::new(RefCell::new(super::scheduler::Scheduler::new()));
    super::scheduler::install(&lua, &sched).unwrap();
    super::world::install(&lua, &world).unwrap();
    lua.load("resumed=false; thread=task.spawn(function() workspace:WaitForChild('future',60); resumed=true end)")
        .exec().unwrap();
    let report = super::scheduler::step(&lua, &sched, Instant::now(), Duration::from_millis(100));
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    lua.load("task.cancel(thread)").exec().unwrap();
    assert_eq!(super::scheduler::snapshot(&sched).active_tasks, 1);
    let report = super::frame_phases::step(
        &lua,
        &world,
        &sched,
        Instant::now(),
        1.0 / 60.0,
        Duration::from_millis(100),
    )
    .unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(super::scheduler::snapshot(&sched).active_tasks, 0);
    assert!(!lua.globals().get::<bool>("resumed").unwrap());
}

#[test]
fn local_part_mass_defaults_are_typed_modifiable_and_resettable() {
    let world = fixture();
    let lua = Lua::new();
    super::enums::install(&lua).unwrap();
    super::signals::install(&lua, &world).unwrap();
    super::world::install(&lua, &world).unwrap();
    let result: bool = lua
        .load(
            r#"
        local part=Instance.new("Part")
        assert(typeof(part.Shape)=="EnumItem" and part.Shape==Enum.PartType.Block)
        assert(typeof(part.Material)=="EnumItem" and part.Material==Enum.Material.Plastic)
        assert(not part:IsPropertyModified("Shape"))
        assert(not part:IsPropertyModified("Material"))

        part.Shape=Enum.PartType.Ball
        part.Material=Enum.Material.Metal
        assert(part:IsPropertyModified("Shape"))
        assert(part:IsPropertyModified("Material"))
        part:ResetPropertyToDefault("Shape")
        part:ResetPropertyToDefault("Material")
        assert(part.Shape==Enum.PartType.Block and not part:IsPropertyModified("Shape"))
        assert(part.Material==Enum.Material.Plastic and not part:IsPropertyModified("Material"))
        return true
    "#,
        )
        .eval()
        .unwrap();
    assert!(result);

    // This is received state, so an omitted wire value stays unavailable.
    assert!(get_property(&world, "p", "Shape")
        .unwrap_err()
        .to_string()
        .contains("unavailable in snapshot"));
    assert!(get_property(&world, "p", "Material")
        .unwrap_err()
        .to_string()
        .contains("unavailable in snapshot"));
}

#[test]
fn input_binding_client_defaults_resolve_without_replicated_values() {
    let lua = Lua::new();
    let world = fixture();
    super::world::install(&lua, &world).unwrap();
    let binding = crate::rust_host::world::create_instance(&lua, &world, "InputBinding", None).unwrap();
    let id = crate::rust_host::world::instance_id(&binding).unwrap();
    let scale = crate::rust_host::world::get_property(&world, &id, "Vector2Scale").unwrap();
    assert_eq!(scale["$vector2"][0], 1.0);
    let threshold = crate::rust_host::world::get_property(&world, &id, "PressedThreshold").unwrap();
    assert_eq!(threshold.as_f64(), Some(0.5));
}
