use serde_json::{Map, Value};


fn current_usage() -> usize {
    use rove_client_core::memory;
    memory::current_process_rss_bytes().unwrap_or(0) as usize
}

#[test]
fn map_overhead_per_node() {
    // Representative shape of a received instance property map.
    let keys = ["Anchored", "CFrame", "Size", "Color", "Transparency", "CanCollide", "Material"];
    let base = current_usage();
    let mut maps: Vec<Map<String, Value>> = Vec::with_capacity(7000);
    for _ in 0..7000 {
        let mut m = Map::new();
        for k in keys {
            m.insert(k.to_string(), Value::Bool(true));
        }
        maps.push(m);
    }
    let used = current_usage() - base;
    println!("per-node bytes = {:.1}", used as f64 / 7000.0);
    println!("map struct = {}", std::mem::size_of::<Map<String, Value>>());
    drop(maps);
}

#[test]
fn prune_far_drops_only_pristine_far_parts() {
    use crate::rust_host::world::{World, WorldRef};
    use rove_client_core::{schema::Schema, world::WorldStore};
    use std::{cell::RefCell, collections::HashMap, rc::Rc};
    let mut store = WorldStore::from_snapshot(&serde_json::json!({
        "version":1,"sequence":0,"nodes":[]
    })).unwrap();
    store.apply_delta(&serde_json::json!({"version":1,"delta":true,"sequence":7,"actions":[
        {"kind":"create","sequence":6,"node":{"id":"near","class_name":"Part","name":"Near","parent":null,
         "properties":{"Size":{"$vector3":[4,1,4]},"CFrame":{"position":[0,0,0],"rotation_id":2}},"received_creation_complete":true}},
        {"kind":"create","sequence":7,"node":{"id":"far","class_name":"Part","name":"Far","parent":null,
         "properties":{"Size":{"$vector3":[4,1,4]},"CFrame":{"position":[5000,0,0],"rotation_id":2}},"received_creation_complete":true}}
    ]})).unwrap();
    let nodes: serde_json::Value = serde_json::json!({"version":1,"sequence":5,"nodes":[
        {"id":"near","class_name":"Part","name":"Near","parent":null,"server_owned":true,
         "received_creation_complete":true,"properties":{"Size":{"$vector3":[4,1,4]},"CFrame":{"$cframe":{"position":[0,0,0],"rotation_id":2}}}},
        {"id":"far","class_name":"Part","name":"Far","parent":null,"server_owned":true,
         "received_creation_complete":true,"properties":{"Size":{"$vector3":[4,1,4]},"CFrame":{"$cframe":{"position":[5000,0,0],"rotation_id":2}}}}
    ]});
    let world: WorldRef = Rc::new(RefCell::new(World::from_snapshot(&nodes, None).unwrap()));
    let pruned = world.borrow_mut().nodes.prune_far(Some([0.,0.,0.]), 512., 100);
    println!("pruned={pruned}");
    assert_eq!(pruned, 1);
    // Only the far part is a candidate; the near one keeps its wrapper.
    assert!(world.borrow().nodes.peek("near").is_some());
}
