use super::{world, world::World};
use serde_json::json;
use std::{cell::RefCell, rc::Rc};

#[test]
fn print_struct_sizes() {
    println!("Node={}", std::mem::size_of::<world::Node>());
    println!("NodeData={}", std::mem::size_of::<world::NodeData>());
    let _ = (json!({}), Rc::new(RefCell::new(World::from_snapshot(&json!({"version":1,"nodes":[],"services":{}}), None).unwrap())));
}
