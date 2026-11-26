use super::world::World;
use super::*;
use serde_json::json;
use std::{cell::RefCell, rc::Rc};

fn fixture() -> (Lua, WorldRef) {
    let snapshot = json!({
        "version":1,
        "sequence":1,
        "services":{"Workspace":"ws","CoreGui":"core"},
        "nodes":[
            {"id":"game","class_name":"DataModel","name":"game","parent":null,"properties":{}},
            {"id":"ws","class_name":"Workspace","name":"Workspace","parent":"game","properties":{}},
            {"id":"part","class_name":"Part","name":"Part","parent":"ws","properties":{}},
            {"id":"script","class_name":"LocalScript","name":"LocalScript","parent":"ws","properties":{}},
            {"id":"core","class_name":"CoreGui","name":"CoreGui","parent":"game","properties":{}},
            {"id":"core_script","class_name":"ModuleScript","name":"CoreModule","parent":"core","properties":{}},
            {"id":"nil_script","class_name":"Script","name":"NilScript","parent":null,"properties":{}},
            {"id":"nil_part","class_name":"Folder","name":"NilFolder","parent":null,"properties":{}}
        ]
    });
    let world = Rc::new(RefCell::new(World::from_snapshot(&snapshot, None).unwrap()));
    let lua = Lua::new();
    world::install(&lua, &world).unwrap();
    super::super::closures::install(&lua).unwrap();
    install(&lua, &world).unwrap();
    for id in ["part", "nil_part"] {
        lua.globals()
            .set(id, world::instance(&lua, &world, id).unwrap())
            .unwrap();
    }
    (lua, world)
}

#[test]
fn cloned_reference_is_a_distinct_wrapper_for_the_same_instance() {
    let (lua, _world) = fixture();
    let result: (bool, bool, bool, bool, bool) = lua
        .load(
            "local copy=cloneref(part); return not rawequal(part,copy), compareinstances(part,copy), cache.iscached(part), cache.cached==cache.iscached, cache.compareinstances==compareinstances",
        )
        .eval()
        .unwrap();
    assert_eq!(result, (true, true, true, true, true));
}

#[test]
fn cache_invalidation_and_replacement_change_canonical_lookup() {
    let (lua, _world) = fixture();
    let result: (bool, bool, bool) = lua
        .load(
            "local copy=cloneref(part); cache.invalidate(part); local invalidated=not cache.iscached(part); cache.replace(part,copy); return invalidated, cache.cached(part), rawequal(workspace:FindFirstChild('Part'),copy)",
        )
        .eval()
        .unwrap();
    assert_eq!(result, (true, true, true));
}

#[test]
fn canonical_wrapper_survives_name_parent_changes_and_collection() {
    let (lua, _world) = fixture();
    let result: (bool, bool, String, bool, bool) = lua
        .load(
            "local held=part; held.Name='Renamed'; held.Parent=game; collectgarbage('collect'); local found=game:FindFirstChild('Renamed'); local same_descendant=false; for _,item in ipairs(game:GetDescendants()) do if item.Name=='Renamed' then same_descendant=rawequal(item,held) end end; return rawequal(held,found), compareinstances(held,found), held.Name, held.Parent==game, same_descendant",
        )
        .eval()
        .unwrap();
    assert_eq!(result, (true, true, "Renamed".to_owned(), true, true));
}

#[test]
fn replacement_rejects_handles_from_another_world() {
    let (lua, world) = fixture();
    let other_snapshot = json!({
        "version":1,"sequence":1,"services":{},
        "nodes":[{"id":"other","class_name":"Part","name":"Other","parent":null,"properties":{}}]
    });
    let other = Rc::new(RefCell::new(
        World::from_snapshot(&other_snapshot, None).unwrap(),
    ));
    let other_value = world::instance(&lua, &other, "other").unwrap();
    lua.globals().set("other_part", other_value).unwrap();
    let result: bool = lua
        .load("not pcall(function() cache.replace(part,other_part) end)")
        .eval()
        .unwrap();
    assert!(result);
    assert!(world::instance_belongs_to(
        &Value::UserData(lua.globals().get("part").unwrap()),
        &world
    ));
}

#[test]
fn enumeration_preserves_world_order_and_filters_nil_scripts_and_dead_nodes() {
    let (lua, world) = fixture();
    let result: (Vec<String>, Vec<String>, Vec<String>) = lua
        .load(
            "local function names(items) local out={} for _,item in ipairs(items) do table.insert(out,item.Name) end return out end; return names(getinstances()), names(getnilinstances()), names(getscripts())",
        )
        .eval()
        .unwrap();
    assert_eq!(
        result.0,
        [
            "game",
            "Workspace",
            "Part",
            "LocalScript",
            "CoreGui",
            "CoreModule",
            "NilScript",
            "NilFolder",
            "UserInputService",
            "RunService",
            "CorePackages"
        ]
    );
    assert_eq!(result.1, ["game", "NilScript", "NilFolder"]);
    assert_eq!(result.2, ["LocalScript", "NilScript"]);

    let _: () = lua.load("part:Destroy()").eval().unwrap();
    let after_destroy: Vec<String> = lua
        .load("local out={} for _,item in ipairs(getinstances()) do table.insert(out,item.Name) end return out")
        .eval()
        .unwrap();
    assert!(!after_destroy.iter().any(|name| name == "Part"));
    assert!(!world::cached_instance_ids(&lua, &world).unwrap().is_empty());
}

#[test]
fn cache_buckets_are_world_scoped_even_when_node_ids_match() {
    let (lua, world_a) = fixture();
    let snapshot_b = json!({
        "version":1,"sequence":1,"services":{},
        "nodes":[{"id":"part","class_name":"Part","name":"DifferentWorld","parent":null,"properties":{}}]
    });
    let world_b = Rc::new(RefCell::new(
        World::from_snapshot(&snapshot_b, None).unwrap(),
    ));
    let a = world::instance(&lua, &world_a, "part").unwrap();
    let b = world::instance(&lua, &world_b, "part").unwrap();
    assert!(!world::compare_instance_references(&a, &b).unwrap());
    assert!(world::instance_belongs_to(&a, &world_a));
    assert!(world::instance_belongs_to(&b, &world_b));
    assert_eq!(
        a.as_userdata().unwrap().get::<String>("Name").unwrap(),
        "Part"
    );
    assert_eq!(
        b.as_userdata().unwrap().get::<String>("Name").unwrap(),
        "DifferentWorld"
    );
    assert!(world::cached_instance_ids(&lua, &world_a)
        .unwrap()
        .contains(&"part".to_owned()));
    assert!(world::cached_instance_ids(&lua, &world_b)
        .unwrap()
        .contains(&"part".to_owned()));
}

#[test]
fn data_model_workspace_alias_resolves_the_received_workspace() {
    let (lua, _world) = fixture();
    let (same, id): (bool, Option<String>) = lua
        .load("return game.workspace == workspace, game.workspace and game.workspace.Name")
        .eval()
        .unwrap();
    assert!(same);
    assert_eq!(id.as_deref(), Some("Workspace"));
}

#[test]
fn invalidation_removes_registry_visible_wrapper_and_lookup_recaches_it() {
    let (lua, _world) = fixture();
    let result: (bool, bool, bool) = lua
        .load(r#"
            local value = workspace:FindFirstChild('Part')
            assert(value and cache.iscached(value))
            cache.invalidate(value)
            local function registry_has_part()
                for _, bucket in pairs(getreg()) do
                    if type(bucket) == 'table' then
                        for _, item in pairs(bucket) do
                            if typeof(item) == 'Instance' and string.find(tostring(item), 'Part') then
                                return true
                            end
                        end
                    end
                end
                return false
            end
            local absent = not cache.iscached(value) and not registry_has_part()
            local recached = workspace:FindFirstChild('Part')
            return absent, cache.iscached(recached), registry_has_part()
        "#)
        .eval()
        .unwrap();
    assert_eq!(result, (true, true, true));
}

#[test]
fn weak_cache_does_not_keep_instance_wrappers_alive() {
    let (lua, world) = fixture();
    assert!(world::cached_instance_ids(&lua, &world)
        .unwrap()
        .contains(&"part".to_owned()));
    lua.load("part=nil; collectgarbage('collect')")
        .exec()
        .unwrap();
    lua.load("collectgarbage('collect')").exec().unwrap();
    assert!(!world::cached_instance_ids(&lua, &world)
        .unwrap()
        .contains(&"part".to_owned()));
}

#[test]
fn completed_received_creations_use_catalog_defaults_but_partial_and_explicit_values_win() {
    let snapshot = json!({"version":1,"sequence":1,"nodes":[
        {"id":"game","class_name":"DataModel","name":"game","parent":null,"properties":{}},
        {"id":"complete_bool","class_name":"BoolValue","name":"CompleteBool","parent":"game","received_creation_complete":true,"properties":{}},
        {"id":"partial_bool","class_name":"BoolValue","name":"PartialBool","parent":"game","properties":{}},
        {"id":"explicit_bool","class_name":"BoolValue","name":"ExplicitBool","parent":"game","received_creation_complete":true,"properties":{"Value":true}},
        {"id":"complete_image","class_name":"ImageLabel","name":"CompleteImage","parent":"game","received_creation_complete":true,"properties":{}}
    ]});

    // Eager snapshots materialize only the small set of known omission
    // fallbacks, and never replace a property that the received record has.
    let eager = Rc::new(RefCell::new(World::from_snapshot(&snapshot, None).unwrap()));
    assert_eq!(world::get_property(&eager, "complete_bool", "Value").unwrap(), json!(false));
    assert_eq!(world::get_property(&eager, "complete_image", "Active").unwrap(), json!(false));
    assert_eq!(world::get_property(&eager, "explicit_bool", "Value").unwrap(), json!(true));
    // Value holders resolve their typed empty `Value` even on partial
    // creates; a partial Game has to answer `nil.X` reads.
    assert_eq!(world::get_property(&eager, "partial_bool", "Value").unwrap(), json!(false));

    // The shared projected path resolves the same catalog defaults lazily and
    // preserves the same explicit-value and incomplete-record rules.
    use crate::{engine_world_projection::ProjectionContext, engine_world_view::SharedWorldView};
    use rove_client_core::{schema::Schema, world::WorldStore};
    use std::collections::HashMap;
    let store = WorldStore::from_snapshot(&snapshot).unwrap();
    let schema = Schema {
        version: 3, flags: 0, enums: vec![], classes: vec![], properties: vec![],
        events: vec![], strings: vec![], names: vec![], lr_types: vec![],
        parent_property_id: 0, decoded_bytes: 0,
    };
    let view = SharedWorldView::projected(
        store.view(),
        ProjectionContext::new(&schema, HashMap::new()).unwrap(),
    );
    let projected = Rc::new(RefCell::new(
        World::from_snapshot(&json!({"version":1,"sequence":1,"nodes":[]}), Some(&view)).unwrap(),
    ));
    assert_eq!(world::get_property(&projected, "complete_bool", "Value").unwrap(), json!(false));
    assert_eq!(world::get_property(&projected, "complete_image", "Active").unwrap(), json!(false));
    assert_eq!(world::get_property(&projected, "explicit_bool", "Value").unwrap(), json!(true));
    assert_eq!(world::get_property(&projected, "partial_bool", "Value").unwrap(), json!(false));
}
