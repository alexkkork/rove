use super::{apply_engine_owned_parts, apply_engine_poses, install};
use crate::rust_host::{
    cframe, enums, numeric, signals,
    world::{self, World, WorldRef},
};
use mlua::Lua;
use serde_json::json;
use std::{cell::RefCell, rc::Rc};

#[path = "movement_shared_tests.rs"]
mod shared_tests;

fn fixture() -> WorldRef {
    let snapshot = json!({"version":1,"sequence":10,"local_player":"player",
    "services":{"Workspace":"ws","Players":"players"},"nodes":[
        {"id":"game","class_name":"DataModel","name":"game","parent":null,"properties":{}},
        {"id":"ws","class_name":"Workspace","name":"Workspace","parent":"game","properties":{"Gravity":196.2}},
        {"id":"players","class_name":"Players","name":"Players","parent":"game","properties":{}},
        {"id":"player","class_name":"Player","name":"LocalPlayer","parent":"players","properties":{"Character":{"$ref":"char"}}},
        {"id":"char","class_name":"Model","name":"Character","parent":"ws","properties":{"PrimaryPart":{"$ref":"root"}}},
        {"id":"hum","class_name":"Humanoid","name":"Humanoid","parent":"char","properties":{"Health":100,"MaxHealth":100}},
        {"id":"root","class_name":"Part","name":"HumanoidRootPart","parent":"char","property_revisions":{"CFrame":10,"Anchored":10,"NetworkOwnerV3":10},"properties":{"Anchored":false,"NetworkOwnerV3":7,"PivotOffset":{"$cframe":{"position":[0,0,0],"rotation":[1,0,0,0,1,0,0,0,1]}},"CFrame":{"$cframe":{"position":[0,3,0],"rotation":[1,0,0,0,1,0,0,0,1]}},"Position":{"$vector3":[0,3,0]},"AssemblyLinearVelocity":{"$vector3":[0,0,0]},"AssemblyAngularVelocity":{"$vector3":[0,0,0]}}},
        {"id":"part","class_name":"Part","name":"OtherPart","parent":"char","properties":{"Anchored":false,"CFrame":{"$cframe":{"position":[2,3,0],"rotation":[1,0,0,0,1,0,0,0,1]}},"Position":{"$vector3":[2,3,0]}}},
        {"id":"loose","class_name":"Part","name":"Loose","parent":"ws","property_revisions":{"CFrame":11,"Anchored":10,"NetworkOwnerV3":10},"properties":{"Anchored":false,"NetworkOwnerV3":7,"CFrame":{"$cframe":{"position":[30,0,0],"rotation":[1,0,0,0,1,0,0,0,1]}},"Position":{"$vector3":[30,0,0]}}}
    ]});
    Rc::new(RefCell::new(World::from_snapshot(&snapshot, None).unwrap()))
}

fn runtime() -> (Lua, WorldRef) {
    let lua = Lua::new();
    let world = fixture();
    signals::install(&lua, &world).unwrap();
    world::install(&lua, &world).unwrap();
    numeric::install(&lua).unwrap();
    enums::install(&lua).unwrap();
    cframe::install(&lua).unwrap();
    install(&lua, &world).unwrap();
    (lua, world)
}

#[test]
fn humanoid_move_and_move_to_preserve_local_intent_shapes() {
    let (lua, world) = runtime();
    let mutation_cursor = world.borrow().effects["mutations"]
        .as_array()
        .unwrap()
        .len();
    lua.load("local p=game:GetService('Players'):FindFirstChild('LocalPlayer'); local h=p.Character:FindFirstChildOfClass('Humanoid'); h:Move(Vector3.new(1,0,0)); h:MoveTo(Vector3.new(4,0,2))").exec().unwrap();
    let intents = world.borrow().effects["movement_intents"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(
        intents[0],
        json!({"kind":"Move","instance_id":"hum","vector":[1.,0.,0.],"relative_to_camera":false,"sequence":1,"mutation_cursor":mutation_cursor})
    );
    assert_eq!(
        intents[1],
        json!({"kind":"MoveTo","instance_id":"hum","vector":[4.,0.,2.],"sequence":2,"mutation_cursor":mutation_cursor+2})
    );
}

#[test]
fn move_to_part_validates_before_mutating_properties_or_queue() {
    let (lua, world) = runtime();
    lua.load(r#"
        h=game:GetService('Players'):FindFirstChild('LocalPlayer').Character.Humanoid
        h:MoveTo(Vector3.new(4,0,2),workspace.Loose)
        assert(h.WalkToPart==workspace.Loose and h.WalkToPoint==Vector3.new(4,0,2))
        local dead=Instance.new('Part');dead:Destroy()
        for _,target in {workspace,dead,{},false,12} do
            assert(not pcall(function() h:MoveTo(Vector3.new(100,0,0),target) end))
        end
        assert(h.WalkToPart==workspace.Loose and h.WalkToPoint==Vector3.new(4,0,2))
    "#).exec().unwrap();
    assert_eq!(world.borrow().effects["movement_intents"].as_array().unwrap().len(), 1);
    assert_eq!(world.borrow().effects["movement_intents"][0]["part_id"], "loose");
    super::limit_actor_intents(&lua, 0);
    lua.load("assert(not pcall(function() h:MoveTo(Vector3.new(5,0,0),nil) end));assert(h.WalkToPart==workspace.Loose and h.WalkToPoint==Vector3.new(4,0,2))").exec().unwrap();
    super::limit_actor_intents(&lua, 256);
    lua.load("h:MoveTo(Vector3.new(5,0,0),nil);assert(h.WalkToPart==nil and h.WalkToPoint==Vector3.new(5,0,0))").exec().unwrap();
    assert_eq!(world.borrow().effects["movement_intents"].as_array().unwrap().len(), 2);
}

#[test]
fn walk_target_setters_validate_and_preflight_only_changed_local_character_values() {
    let (lua, world) = runtime();
    lua.load("h=game:GetService('Players'):FindFirstChild('LocalPlayer').Character.Humanoid;h:MoveTo(Vector3.new(4,0,2),workspace.Loose)").exec().unwrap();
    super::limit_actor_intents(&lua, 1);
    lua.load(r#"
        h.WalkToPoint=Vector3.new(4,0,2);h.WalkToPart=workspace.Loose
        assert(not pcall(function() h.WalkToPoint=Vector3.new(10,0,0) end))
        assert(not pcall(function() h.WalkToPart=nil end))
        assert(not pcall(function() h.WalkToPoint=Vector3.new(0/0,0,0) end))
        assert(not pcall(function() h.WalkToPoint=nil end))
        assert(not pcall(function() h.WalkToPart=workspace end))
        assert(h.WalkToPoint==Vector3.new(4,0,2) and h.WalkToPart==workspace.Loose)
        local npc=Instance.new('Humanoid',workspace)
        npc.WalkToPoint=Vector3.new(20,0,0);npc.WalkToPart=workspace.Loose
    "#).exec().unwrap();
    assert_eq!(world.borrow().effects["movement_intents"].as_array().unwrap().len(), 1);
    super::limit_actor_intents(&lua, 256);
    lua.load("h.WalkToPoint=Vector3.new(5,0,0);h.WalkToPart=nil").exec().unwrap();
    let w = world.borrow();
    let commands = w.effects["movement_intents"].as_array().unwrap();
    assert_eq!(commands.len(), 3);
    for (row, key) in commands[1..].iter().zip(["WalkToPoint", "WalkToPart"]) {
        assert_eq!(row["kind"], "CancelMoveTo");
        assert_eq!(row["property"], key);
        assert_eq!(row["player_id"], "player");
        assert_eq!(row["character_id"], "char");
    }
}

#[test]
fn root_cframe_and_part_pivot_preflight_before_local_pose_changes() {
    let (lua, world) = runtime();
    lua.load(r#"
        c=game:GetService('Players'):FindFirstChild('LocalPlayer').Character
        r=c.HumanoidRootPart
        old=r.CFrame
        local weld=Instance.new('WeldConstraint',c);weld.Part0=r;weld.Part1=c.OtherPart
        other=c.OtherPart.CFrame
    "#).exec().unwrap();
    super::limit_actor_intents(&lua, 0);
    lua.load(r#"
        assert(not pcall(function() r.CFrame=CFrame.new(20,30,40) end))
        assert(not pcall(function() r:PivotTo(CFrame.new(20,30,40)) end))
        assert(r.CFrame==old and c.OtherPart.CFrame==other)
        workspace.Loose.CFrame=CFrame.new(30,40,50)
    "#).exec().unwrap();
    assert!(world.borrow().effects.get("movement_intents").is_none_or(|v| v.as_array().unwrap().is_empty()));
    super::limit_actor_intents(&lua, 256);
    lua.load(r#"
        for _,write in {
            function() r.CFrame=CFrame.new(math.huge,0,0) end,
            function() r.CFrame=CFrame.new(1e10,0,0) end,
            function() r.CFrame=CFrame.new(0,0,0,0,0,0,0,0,0,0,0,0) end,
            function() r.CFrame=CFrame.new(0,0,0,-1,0,0,0,1,0,0,0,1) end
        } do assert(not pcall(write)) end
        assert(r.CFrame==old and c.OtherPart.CFrame==other)
        r.CFrame=CFrame.new(10,3,0)
        assert(c.OtherPart.Position==Vector3.new(12,3,0))
    "#).exec().unwrap();
    let w = world.borrow();
    let commands = w.effects["movement_intents"].as_array().unwrap();
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0]["kind"], "PivotTo");
    assert_eq!(commands[0]["instance_id"], "char");
    assert_eq!(commands[0]["root_id"], "root");
}

#[test]
fn bulk_root_preflight_rejects_the_entire_batch_and_unrelated_moves_need_no_capacity() {
    let (lua, world) = runtime();
    lua.load(r#"
        c=game:GetService('Players'):FindFirstChild('LocalPlayer').Character
        r=c.HumanoidRootPart;p=c.OtherPart;loose=workspace.Loose
        old=r.CFrame;other=p.CFrame;outside=loose.CFrame
        local w=Instance.new('WeldConstraint',c);w.Part0=r;w.Part1=p
    "#).exec().unwrap();
    super::limit_actor_intents(&lua, 0);
    lua.load(r#"
        assert(not pcall(function()
            workspace:BulkMoveTo({loose,p},{CFrame.new(99,0,0),CFrame.new(20,3,0)})
        end))
        assert(r.CFrame==old and p.CFrame==other and loose.CFrame==outside)
        workspace:BulkMoveTo({loose},{CFrame.new(40,0,0)})
        assert(loose.Position==Vector3.new(40,0,0))
    "#).exec().unwrap();
    assert!(world.borrow().effects.get("movement_intents").is_none_or(|v| v.as_array().unwrap().is_empty()));
    super::limit_actor_intents(&lua, 256);
    lua.load(r#"
        assert(not pcall(function()
            workspace:BulkMoveTo({loose,r},{CFrame.new(99,0,0),CFrame.new(1e10,0,0)})
        end))
        assert(r.CFrame==old and p.CFrame==other and loose.Position==Vector3.new(40,0,0))
        workspace:BulkMoveTo({r},{CFrame.new(10,3,0)})
    "#).exec().unwrap();
    assert_eq!(world.borrow().effects["movement_intents"].as_array().unwrap().len(), 1);
    super::limit_actor_intents(&lua, 1);
    lua.load("workspace:BulkMoveTo({r,loose},{r.CFrame,CFrame.new(50,0,0)});assert(loose.Position==Vector3.new(50,0,0))").exec().unwrap();
    assert_eq!(world.borrow().effects["movement_intents"].as_array().unwrap().len(), 1);
}

#[test]
fn full_move_queue_preserves_previous_input_observation() {
    let (lua, world) = runtime();
    lua.load("h=game:GetService('Players'):FindFirstChild('LocalPlayer').Character.Humanoid;h:Move(Vector3.new(0.5,0,0));assert(h.MoveDirection==Vector3.new(1,0,0))").exec().unwrap();
    super::limit_actor_intents(&lua, 1);
    lua.load("assert(not pcall(function()h:Move(Vector3.new(-1,0,0))end));assert(h.MoveDirection==Vector3.new(1,0,0));assert(not pcall(function()h.MoveDirection=Vector3.zero end))").exec().unwrap();
    assert_eq!(world.borrow().effects["movement_intents"].as_array().unwrap().len(), 1);
}

#[test]
fn target_observation_waits_for_queue_capacity_and_coalesces_pending_edits() {
    let (lua, world)=runtime();
    lua.load("h=game:GetService('Players'):FindFirstChild('LocalPlayer').Character.Humanoid;t=workspace.Loose;h:MoveTo(Vector3.zero,t);t.Position=Vector3.new(40,0,0)").exec().unwrap();
    super::limit_actor_intents(&lua,1);
    super::flush_move_to_target(&lua,&world).unwrap();
    assert_eq!(world.borrow().effects["movement_intents"].as_array().unwrap().len(),1);
    lua.load("t.Position=Vector3.new(50,0,0)").exec().unwrap();
    world.borrow_mut().effects.remove("movement_intents");
    super::flush_move_to_target(&lua,&world).unwrap();
    let commands=world.borrow_mut().effects.remove("movement_intents").unwrap();
    assert_eq!(commands.as_array().unwrap().len(),1);
    assert_eq!(commands[0]["cframe"]["$cframe"]["position"],json!([50.,0.,0.]));
    assert_eq!(commands[0]["request_sequence"],1);
    super::flush_move_to_target(&lua,&world).unwrap();
    assert!(!world.borrow().effects.contains_key("movement_intents"));
    world.borrow_mut().apply_delta(&json!({"version":1,"delta":true,"sequence":12,"actions":[
        {"kind":"set","sequence":12,"id":"loose","fields":{"CFrame":{"$cframe":{"position":[30,0,0],"rotation":[1,0,0,0,1,0,0,0,1]}}}}
    ]}),None).unwrap();
    lua.load("t.Position=Vector3.new(50,0,0)").exec().unwrap();
    super::flush_move_to_target(&lua,&world).unwrap();
    let refreshed=world.borrow_mut().effects.remove("movement_intents").unwrap();
    assert_eq!(refreshed[0]["received_sequence"],12);
    assert_eq!(refreshed[0]["pose_revision"],12);
    assert_eq!(refreshed[0]["cframe"]["$cframe"]["position"],json!([50.,0.,0.]));
    lua.load("h:Move(Vector3.zero);t.Position=Vector3.new(60,0,0)").exec().unwrap();
    world.borrow_mut().effects.remove("movement_intents");
    super::flush_move_to_target(&lua,&world).unwrap();
    assert!(!world.borrow().effects.contains_key("movement_intents"));
}

#[test]
fn root_alias_queue_failure_preserves_the_part_and_incident_joint_offsets() {
    let (lua, world) = runtime();
    lua.load(r#"
        c=game:GetService('Players'):FindFirstChild('LocalPlayer').Character
        r=c.HumanoidRootPart;p=c.OtherPart
        joint=Instance.new('Motor6D',c);joint.C0=CFrame.new(2,0,0);joint.Part0=r;joint.Part1=p
        old=r.CFrame;other=p.CFrame;offset=joint.C0
    "#).exec().unwrap();
    super::limit_actor_intents(&lua, 0);
    lua.load(r#"
        for _,key in {'Position','Orientation','Rotation'} do
            assert(not pcall(function()r[key]=Vector3.new(20,30,40)end))
            assert(r.CFrame==old and p.CFrame==other and joint.C0==offset)
        end
    "#).exec().unwrap();
    assert!(world.borrow().effects.get("movement_intents").is_none_or(|v| v.as_array().unwrap().is_empty()));
    super::limit_actor_intents(&lua, 256);
    lua.load("r.Position=Vector3.new(10,3,0);assert(p.CFrame==other);assert(((r.CFrame*joint.C0).Position-(p.CFrame*joint.C1).Position).Magnitude<1e-5)").exec().unwrap();
    assert_eq!(world.borrow().effects["movement_intents"].as_array().unwrap().len(), 1);
    super::limit_actor_intents(&lua, 1);
    lua.load("r.Position=r.Position;p.Position=Vector3.new(4,3,0);assert(r.Position==Vector3.new(10,3,0))").exec().unwrap();
    assert_eq!(world.borrow().effects["movement_intents"].as_array().unwrap().len(), 1);
}

#[test]
fn humanoid_state_methods_use_feedback_and_queue_local_state_intents() {
    let (lua, world) = runtime();
    world::set_engine_property(&world, "hum", "__HumanoidStateFeedback", json!(5), None).unwrap();
    lua.load("local h=game:GetService('Players'):FindFirstChild('LocalPlayer').Character:FindFirstChildOfClass('Humanoid'); local state=h:GetState(); assert(state.Name=='Freefall' and state.Value==5); assert(h:GetStateEnabled(Enum.HumanoidStateType.Jumping)); h:ChangeState(Enum.HumanoidStateType.Jumping); h:SetStateEnabled(Enum.HumanoidStateType.Swimming,false); assert(not h:GetStateEnabled(Enum.HumanoidStateType.Swimming))").exec().unwrap();
    let intents = world.borrow().effects["movement_intents"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(intents[0]["kind"], json!("ChangeState"));
    assert_eq!(intents[0]["state_value"], json!(3));
    assert_eq!(intents[1]["kind"], json!("SetStateEnabled"));
    assert_eq!(intents[1]["enabled"], json!(false));
}

#[test]
fn translate_by_and_pivot_to_write_local_descendant_transforms() {
    let (lua, world) = runtime();
    let bootstrap_mutations = world.borrow().effects["mutations"]
        .as_array()
        .unwrap()
        .len();
    lua.load("local m=game:GetService('Players'):FindFirstChild('LocalPlayer').Character; m:TranslateBy(Vector3.new(5,0,-1)); m:PivotTo(CFrame.new(20,0,0))").exec().unwrap();
    let root = world::get_property(&world, "root", "CFrame").unwrap();
    assert_eq!(root["$cframe"]["position"], json!([20., 0., 0.]));
    let part = world::get_property(&world, "part", "CFrame").unwrap();
    assert_eq!(part["$cframe"]["position"], json!([22., 0., 0.]));
    assert_eq!(
        world.borrow().effects["mutations"]
            .as_array()
            .unwrap()
            .len(),
        // 16 pose writes for TranslateBy + PivotTo, plus 4 pivot-fallback
        // property reads recorded by the streaming-model pivot path.
        bootstrap_mutations + 20
    );
}

#[test]
fn gravity_and_anchored_are_local_property_mutations() {
    let (lua, world) = runtime();
    let bootstrap_mutations = world.borrow().effects["mutations"]
        .as_array()
        .unwrap()
        .len();
    lua.load("workspace.Gravity=80; game:GetService('Players'):FindFirstChild('LocalPlayer').Character:FindFirstChild('HumanoidRootPart').Anchored=true").exec().unwrap();
    assert_eq!(
        world::get_property(&world, "ws", "Gravity").unwrap(),
        json!(80.0)
    );
    assert_eq!(
        world::get_property(&world, "root", "Anchored").unwrap(),
        json!(true)
    );
    assert_eq!(
        world.borrow().effects["mutations"]
            .as_array()
            .unwrap()
            .len(),
        bootstrap_mutations + 2
    );
}

#[test]
fn engine_pose_requires_current_character_and_all_writes_validate_before_commit() {
    let world = fixture();
    let frame = json!({"$cframe":{"position":[8.,9.,10.],"rotation":[1.,0.,0.,0.,1.,0.,0.,0.,1.]}});
    let stale = json!({"character_id":"other","root_id":"root","frames":[{"id":"root","cframe":frame.clone()}]});
    assert_eq!(
        apply_engine_poses(&world, &stale).unwrap(),
        json!({"status":"stale_character","frames_applied":0})
    );
    let malformed = json!({"character_id":"char","root_id":"root","frames":[{"id":"root","cframe":frame.clone()},{"id":"missing","cframe":frame.clone()}]});
    assert!(apply_engine_poses(&world, &malformed).is_err());
    assert_eq!(
        world::get_property(&world, "root", "CFrame").unwrap()["$cframe"]["position"],
        json!([0, 3, 0])
    );
}

#[test]
fn owned_part_feedback_rejects_stale_property_revisions() {
    let world = fixture();
    let batch = json!({"scope":7,"source_revision":10,"frames":[{"id":"loose","cframe":{"$cframe":{"position":[8.,9.,10.],"rotation":[1.,0.,0.,0.,1.,0.,0.,0.,1.]}},"linear_velocity":[0.,0.,0.],"angular_velocity":[0.,0.,0.]}]});
    assert_eq!(
        apply_engine_owned_parts(&world, &batch, 7).unwrap(),
        json!({"status":"stale_authority","frames_applied":0})
    );
    assert_eq!(
        world::get_property(&world, "loose", "CFrame").unwrap()["$cframe"]["position"],
        json!([30, 0, 0])
    );
}

#[test]
fn gravity_validation_and_queue_limit_do_not_partially_write_workspace() {
    let (lua, world) = runtime();
    let before = world::get_property(&world, "ws", "Gravity").unwrap();
    lua.load("for _,v in {math.huge,0/0,-1,1000001,'heavy'} do assert(not pcall(function() workspace.Gravity=v end)) end").exec().unwrap();
    assert_eq!(world::get_property(&world, "ws", "Gravity").unwrap(), before);
    super::limit_actor_intents(&lua, 0);
    assert!(!lua.load("return pcall(function() workspace.Gravity=0 end)").eval::<bool>().unwrap());
    assert_eq!(world::get_property(&world, "ws", "Gravity").unwrap(), before);
    assert!(world.borrow().effects.get("movement_intents").is_none());
    super::limit_actor_intents(&lua, 1);
    lua.load("workspace.Gravity=0").exec().unwrap();
    assert_eq!(world.borrow().effects["movement_intents"][0]["kind"], "Gravity");
    assert_eq!(world.borrow().effects["movement_intents"][0]["received_sequence"], 10);
}

#[test]
fn walk_speed_validation_and_queue_capacity_precede_property_mutation() {
    let (lua, world) = runtime();
    let before = world::get_property(&world, "hum", "WalkSpeed").ok();
    lua.load("h=game:GetService('Players'):FindFirstChild('LocalPlayer').Character.Humanoid; for _,v in {math.huge,0/0,-1,10001,'fast'} do assert(not pcall(function() h.WalkSpeed=v end)) end").exec().unwrap();
    assert_eq!(world::get_property(&world, "hum", "WalkSpeed").ok(), before);
    super::limit_actor_intents(&lua, 0);
    assert!(!lua.load("return pcall(function() h.WalkSpeed=24 end)").eval::<bool>().unwrap());
    assert_eq!(world::get_property(&world, "hum", "WalkSpeed").ok(), before);
    assert!(world.borrow().effects.get("movement_intents").is_none());
    super::limit_actor_intents(&lua, 1);
    lua.load("h.WalkSpeed=24").exec().unwrap();
    assert_eq!(world.borrow().effects["movement_intents"][0]["value"], 24.);
    assert_eq!(world.borrow().effects["movement_intents"][0]["received_sequence"], 10);
    world.borrow_mut().effects.remove("movement_intents");
    lua.load("h.Parent=workspace;h.WalkSpeed=12").exec().unwrap();
    assert!(world.borrow().effects.get("movement_intents").is_none());
    assert_eq!(world::get_property(&world, "hum", "WalkSpeed").unwrap(), 12.);
}

#[test]
fn root_velocity_write_is_typed_and_queue_limit_precedes_mutation() {
    let (lua,world)=runtime();
    let before=world::get_property(&world,"root","AssemblyLinearVelocity").unwrap();
    world.borrow_mut().effects.insert("movement_intents".into(),json!((0..256).map(|_|json!({})).collect::<Vec<_>>()));
    let ok:bool=lua.load("return pcall(function() game:GetService('Players'):FindFirstChild('LocalPlayer').Character.HumanoidRootPart.AssemblyLinearVelocity=Vector3.new(3,4,5) end)").eval().unwrap();
    assert!(!ok);assert_eq!(world::get_property(&world,"root","AssemblyLinearVelocity").unwrap(),before);
    world.borrow_mut().effects.remove("movement_intents");
    lua.load("game:GetService('Players'):FindFirstChild('LocalPlayer').Character.HumanoidRootPart.AssemblyLinearVelocity=Vector3.new(3,4,5)").exec().unwrap();
    let w=world.borrow();let rows=w.effects["movement_intents"].as_array().unwrap();
    assert_eq!(rows.len(),1);assert_eq!(rows[0]["root_id"],"root");assert_eq!(rows[0]["vector"],json!([3.,4.,5.]));
}
#[test]
fn noncharacter_velocity_edits_do_not_queue_character_movement() {
    let (lua,world)=runtime();
    lua.load("workspace.Loose.AssemblyLinearVelocity=Vector3.new(3,4,5)").exec().unwrap();
    assert!(world.borrow().effects.get("movement_intents").is_none());
    assert_eq!(world::get_property(&world,"loose","AssemblyLinearVelocity").unwrap(),json!({"$vector3":[3.,4.,5.]}));
}

#[test]
fn actor_intent_allowance_rejects_jump_before_property_mutation() {
    let (lua,world)=runtime();
    let before=world::get_property(&world,"hum","Jump").ok();
    super::limit_actor_intents(&lua,0);
    let success:bool=lua.load("return pcall(function() game:GetService('Players'):FindFirstChild('LocalPlayer').Character.Humanoid.Jump=true end)").eval().unwrap();
    assert!(!success);assert_eq!(world::get_property(&world,"hum","Jump").ok(),before);
    assert!(world.borrow().effects.get("movement_intents").is_none());
    super::limit_actor_intents(&lua,1);
    lua.load("game:GetService('Players'):FindFirstChild('LocalPlayer').Character.Humanoid.Jump=true").exec().unwrap();
    assert_eq!(world.borrow().effects["movement_intents"].as_array().unwrap().len(),1);
}
