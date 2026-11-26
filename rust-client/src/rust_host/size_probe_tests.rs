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
