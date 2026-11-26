use super::{
    services, signals,
    world::{self, World, WorldRef},
};
use mlua::{Lua, Value};
use serde_json::json;
use std::{cell::RefCell, rc::Rc};

fn fixture() -> WorldRef {
    let snapshot = json!({"version":1,"sequence":1,"services":{
        "Players":"players","HttpService":"http","CollectionService":"collection",
        "RunService":"run","TestService":"test","Debris":"debris","Stats":"stats"
    },"nodes":[
        {"id":"game","class_name":"DataModel","name":"game","parent":null,"properties":{},"server_owned":true},
        {"id":"players","class_name":"Players","name":"Players","parent":"game","properties":{},"server_owned":true},
        {"id":"http","class_name":"HttpService","name":"HttpService","parent":"game","properties":{},"server_owned":true},
        {"id":"collection","class_name":"CollectionService","name":"CollectionService","parent":"game","properties":{},"server_owned":true},
        {"id":"run","class_name":"RunService","name":"RunService","parent":"game","properties":{},"server_owned":true},
        {"id":"test","class_name":"TestService","name":"TestService","parent":"game","properties":{},"server_owned":true},
        {"id":"debris","class_name":"Debris","name":"Debris","parent":"game","properties":{},"server_owned":true},
        {"id":"stats","class_name":"Stats","name":"Stats","parent":"game","properties":{},"server_owned":true},
        {"id":"network","class_name":"Folder","name":"Network","parent":"stats","properties":{},"server_owned":true},
        {"id":"server_stats","class_name":"Folder","name":"ServerStatsItem","parent":"network","properties":{},"server_owned":true},
        {"id":"ping","class_name":"StatsItem","name":"Data Ping","parent":"server_stats","properties":{},"server_owned":true},
        {"id":"player","class_name":"Player","name":"Tester","parent":"players","properties":{"UserId":42},"server_owned":true},
        {"id":"part","class_name":"Part","name":"TaggedPart","parent":"game","properties":{},"server_owned":true,"collection_tags_known":true,"collection_tags":[]}
    ]});
    Rc::new(RefCell::new(World::from_snapshot(&snapshot, None).unwrap()))
}

fn setup() -> (Lua, WorldRef) {
    let lua = Lua::new();
    let world = fixture();
    world::install(&lua, &world).unwrap();
    signals::install(&lua, &world).unwrap();
    services::install(&lua, &world).unwrap();
    (lua, world)
}

#[test]
fn http_service_json_and_url_methods_use_bounded_codec() {
    let (lua, _world) = setup();
    let encoded: String = lua
        .load(
            r#"
        local http = game:GetService("HttpService")
        return http:JSONEncode({ok=true, values={1, 2}})
    "#,
        )
        .eval()
        .unwrap();
    assert_eq!(encoded, r#"{"ok":true,"values":[1,2]}"#);
    let url: String = lua
        .load(r#"return game:GetService("HttpService"):UrlEncode("hi there!")"#)
        .eval()
        .unwrap();
    assert_eq!(url, "hi%20there%21");
    let decoded: Value = lua
        .load(r#"return game:GetService("HttpService"):JSONDecode("[1,2]")"#)
        .eval()
        .unwrap();
    assert!(matches!(decoded, Value::Table(_)));
}

#[test]
fn collection_service_preserves_unknown_tag_state_and_records_local_changes() {
    let (lua, world) = setup();
    let bootstrap_mutations = world.borrow().effects["mutations"]
        .as_array()
        .unwrap()
        .len();
    let unknown = lua.load(r#"return game:GetService("CollectionService"):GetTags(game:GetService("Players"):GetPlayers()[1])"#).eval::<Value>();
    assert!(unknown.is_err());
    lua.load(r#"game:GetService("CollectionService"):AddTag(game:FindFirstChild("TaggedPart"), "test-tag")"#).exec().unwrap();
    let tagged: bool = lua.load(r#"return game:GetService("CollectionService"):HasTag(game:FindFirstChild("TaggedPart"), "test-tag")"#).eval().unwrap();
    assert!(tagged);
    assert_eq!(
        world.borrow().effects["mutations"][bootstrap_mutations],
        json!({"kind":"set_tags","id":"part","tags":["test-tag"]})
    );
}

#[test]
fn players_and_runservice_reflect_fixture_and_local_client_mode() {
    let (lua, _world) = setup();
    let count: i64 = lua
        .load(r#"return #game:GetService("Players"):GetPlayers()"#)
        .eval()
        .unwrap();
    assert_eq!(count, 1);
    let user_id: i64 = lua
        .load(r#"return game:GetService("Players"):GetPlayerByUserId(42).UserId"#)
        .eval()
        .unwrap();
    assert_eq!(user_id, 42);
    let is_client: bool = lua
        .load(r#"return game:GetService("RunService"):IsClient()"#)
        .eval()
        .unwrap();
    let is_server: bool = lua
        .load(r#"return game:GetService("RunService"):IsServer()"#)
        .eval()
        .unwrap();
    assert!(is_client);
    assert!(!is_server);
}

#[test]
fn user_settings_creates_only_local_defaults() {
    let (lua, world) = setup();
    let sensitivity: f64 = lua
        .load(r#"return UserSettings():GetService("UserGameSettings").MouseSensitivity"#)
        .eval()
        .unwrap();
    assert_eq!(sensitivity, 1.0);
    let settings_id = world.borrow().services["UserSettings"].clone();
    assert!(!world.borrow().nodes[&settings_id].server_owned);
    let writable: bool = lua
        .load(
            r#"
            local settings = UserSettings():GetService("UserGameSettings")
            settings.MouseSensitivity = 1.5
            return settings.MouseSensitivity == 1.5
        "#,
        )
        .eval()
        .unwrap();
    assert!(writable);
    let readonly = lua
        .load(
            r#"
            local settings = UserSettings():GetService("UserGameSettings")
            settings.IsUsingCameraYInverted = true
        "#,
        )
        .exec();
    assert!(readonly.is_err());
}

#[test]
fn stats_reads_only_fixture_measured_rtt() {
    let (lua, world) = setup();
    world.borrow_mut().snapshot["session_stats"] = json!({"network_rtt_ms":28.375});
    let value: f64 = lua.load(r#"return game:GetService("Stats").Network:FindFirstChild("ServerStatsItem"):FindFirstChild("Data Ping"):GetValue()"#).eval().unwrap();
    assert_eq!(value, 28.375);
}

#[test]
fn stats_globals_use_the_same_service_and_only_actual_session_values() {
    let (lua, world) = setup();
    lua.load(
        r#"
        assert(Stats == stats and Stats() == game:GetService('Stats'))
        local ping = stats().Network.ServerStatsItem['Data Ping']
        assert(not pcall(function() return ping:GetValue() end))
    "#,
    )
    .exec()
    .unwrap();
    world.borrow_mut().snapshot["session_stats"] = json!({"network_rtt_ms":31.25});
    assert_eq!(
        lua.load("return Stats().Network.ServerStatsItem['Data Ping']:GetValue()")
            .eval::<f64>()
            .unwrap(),
        31.25
    );
}

#[test]
fn stats_globals_create_one_local_service_when_snapshot_has_none() {
    let world = Rc::new(RefCell::new(
        World::from_snapshot(&json!({"version":1,"nodes":[],"services":{}}), None).unwrap(),
    ));
    let lua = Lua::new();
    signals::install(&lua, &world).unwrap();
    world::install(&lua, &world).unwrap();
    services::install(&lua, &world).unwrap();
    lua.load(
        r#"
        assert(game:FindService('Stats') == nil)
        local service = stats()
        assert(service == Stats() and service == game:GetService('Stats'))
        assert(service:IsA('Stats') and service.Parent == game)
    "#,
    )
    .exec()
    .unwrap();
    let state = world.borrow();
    let nodes: Vec<_> = state
        .nodes
        .values()
        .filter(|node| node.class_name == "Stats")
        .collect();
    assert_eq!(nodes.len(), 1);
    assert!(!nodes[0].server_owned);
    assert!(state.snapshot.get("session_stats").is_none());
}

#[test]
fn debris_runs_subtree_events_and_releases_pending_scripts() {
    use super::{bindables, scheduler};
    use std::time::{Duration, Instant};
    let (lua, world) = setup();
    let sched=Rc::new(RefCell::new(scheduler::Scheduler::new()));
    scheduler::install(&lua,&sched).unwrap();
    bindables::install(&lua,&world).unwrap();
    lua.load(r#"
        root=Instance.new('Folder',game); root.Name='temporary'
        child=Instance.new('BindableFunction',root); child.Name='callback'
        leaf=Instance.new('Part',child); leaf.Name='leaf'
        destroying=0; removed={}; invoke_done=false; child_done=false
        for _,item in ipairs({root,child,leaf}) do item.Destroying:Connect(function() destroying+=1 end) end
        connection=leaf.Changed:Connect(function() end)
        game.DescendantRemoving:Connect(function(item) table.insert(removed,item.Name) end)
        task.spawn(function() local ok,err=pcall(function() child:Invoke() end); invoke_done=not ok and string.find(err,'destroyed')~=nil end)
        task.spawn(function() child_done=root:WaitForChild('never')==nil end)
        game:GetService('Debris'):AddItem(root,0)
    "#).exec().unwrap();
    assert!(scheduler::step(&lua,&sched,Instant::now(),Duration::from_millis(100)).errors.is_empty());
    services::tick(&lua,&world).unwrap();
    let report=scheduler::step(&lua,&sched,Instant::now(),Duration::from_millis(100));
    assert!(report.errors.is_empty(),"{:?}",report.errors);
    lua.load("assert(destroying==3 and table.concat(removed,',')=='temporary,callback,leaf'); assert(invoke_done and child_done and not connection.Connected); assert(root.Parent==nil and child.Parent==nil and leaf.Parent==nil); root:Destroy()")
        .exec().unwrap();
    let world=world.borrow();
    let destroys=world.effects["mutations"].as_array().unwrap().iter().filter(|row|row["kind"]=="destroy").collect::<Vec<_>>();
    assert_eq!(destroys.len(),1);
    assert_eq!(destroys[0]["ids"].as_array().unwrap().len(),3);
}

#[test]
fn debris_overflow_uses_full_destruction_and_keeps_new_items_queued() {
    use super::scheduler;
    use std::time::{Duration,Instant};
    let (lua,world)=setup();
    let sched=Rc::new(RefCell::new(scheduler::Scheduler::new()));
    scheduler::install(&lua,&sched).unwrap();
    lua.load(r#"
        debris=game:GetService('Debris'); oldest=Instance.new('Folder',game)
        child=Instance.new('Part',oldest); destroyed=0
        child.Destroying:Connect(function() destroyed+=1 end)
        connection=child.Changed:Connect(function() end)
        debris:AddItem(oldest,60)
        for i=1,1000 do newest=Instance.new('Part',game); debris:AddItem(newest,60) end
    "#).exec().unwrap();
    let report=scheduler::step(&lua,&sched,Instant::now(),Duration::from_millis(100));
    assert!(report.errors.is_empty(),"{:?}",report.errors);
    lua.load("assert(destroyed==1 and not connection.Connected and oldest.Parent==nil and child.Parent==nil and newest.Parent==game)").exec().unwrap();
    let world=world.borrow();
    assert_eq!(world.effects["mutations"].as_array().unwrap().iter().filter(|row|row["kind"]=="destroy").count(),1);
}

#[test]
fn debris_zero_lifetime_destroys_the_local_item_on_tick() {
    let (lua, world) = setup();
    lua.load(
        r#"
        local item = Instance.new("Part")
        game:GetService("Debris"):AddItem(item, 0)
    "#,
    )
    .exec()
    .unwrap();
    services::tick(&lua, &world).unwrap();
    assert!(world
        .borrow()
        .nodes
        .values()
        .any(|node| node.class_name == "Part" && node.destroyed));
}

#[test]
fn local_player_unknown_is_nil_and_user_settings_use_internal_local_nodes() {
    let (lua, world) = setup();
    let unknown: Value = lua
        .load("return game:GetService('Players').LocalPlayer")
        .eval()
        .unwrap();
    assert!(matches!(unknown, Value::Nil));
    let (settings_name, game_name, sensitivity): (String, String, f64) = lua
        .load(
            r#"
        local settings=UserSettings()
        local gameSettings=settings:FindFirstChild('GameSettings')
        return settings.Name, gameSettings.Name, gameSettings.MouseSensitivity
    "#,
        )
        .eval()
        .unwrap();
    assert_eq!(settings_name, "GlobalBasicSettings");
    assert_eq!(game_name, "GameSettings");
    assert_eq!(sensitivity, 1.0);
    assert!(world
        .borrow()
        .nodes
        .values()
        .any(|n| n.class_name == "UserSettings"));
    assert!(world
        .borrow()
        .nodes
        .values()
        .any(|n| n.class_name == "UserGameSettings"));
}

#[test]
fn get_mouse_uses_valid_local_player_metadata_not_a_property_copy() {
    let snapshot = json!({
        "version":1,"sequence":0,"local_player":"local-player",
        "services":{"Players":"players"},
        "nodes":[
            {"id":"game","class_name":"DataModel","name":"game","server_owned":true},
            {"id":"players","class_name":"Players","name":"Players","parent":"game","server_owned":true,"properties":{}},
            {"id":"local-player","class_name":"Player","name":"Local","parent":"players","server_owned":true,"properties":{"UserId":42}},
            {"id":"other-player","class_name":"Player","name":"Other","parent":"players","server_owned":true,"properties":{"UserId":99}}
        ]
    });
    let world = Rc::new(RefCell::new(World::from_snapshot(&snapshot, None).unwrap()));
    let lua = Lua::new();
    world::install(&lua, &world).unwrap();
    signals::install(&lua, &world).unwrap();
    services::install(&lua, &world).unwrap();

    // The Players node deliberately has no LocalPlayer property. Identity is
    // session metadata, while the Player's own received UserId stays intact.
    let (user_id, mouse_name, mouse_class, icon_empty, filter_nil, same_mouse,
        has_button_signal, public_create_denied): (u64, String, String, bool, bool, bool, bool, bool) = lua
        .load(r#"
            local player = game:GetService('Players').LocalPlayer
            local mouse = player:GetMouse()
            return player.UserId, mouse.Name, mouse.ClassName,
                mouse.Icon == '', mouse.TargetFilter == nil,
                player:GetMouse() == mouse, mouse.Button1Down ~= nil,
                not pcall(function() Instance.new('PlayerMouse') end)
        "#)
        .eval()
        .unwrap();
    assert_eq!(user_id, 42);
    assert_eq!(mouse_name, "Mouse");
    assert_eq!(mouse_class, "PlayerMouse");
    assert!(icon_empty && filter_nil && same_mouse && has_button_signal);
    assert!(public_create_denied, "PlayerMouse must stay non-creatable in Instance.new");

    let other = world::instance(&lua, &world, "other-player").unwrap();
    lua.globals().set("other_player", other).unwrap();
    let error = lua
        .load("return other_player:GetMouse()")
        .eval::<Value>()
        .unwrap_err()
        .to_string();
    assert!(error.contains("GetMouse requires LocalPlayer"), "{error}");
}

#[test]
fn datamodel_is_loaded_reflects_only_received_replication_state() {
    let (lua, world) = setup();
    let before: bool = lua.load("return game:IsLoaded()").eval().unwrap();
    assert!(!before);
    world.borrow_mut().snapshot["initial_replication_complete"] = json!(true);
    let after: bool = lua.load("return game:IsLoaded()").eval().unwrap();
    assert!(after);
}

#[test]
fn engine_globals_are_local_singletons_with_explicit_unavailable_operations() {
    let (lua, world) = setup();
    let bootstrap_mutations = world.borrow().effects.get("mutations").cloned();
    lua.load(r#"
        local global, plugins = settings(), PluginManager()
        assert(global == settings() and plugins == PluginManager())
        assert(global.Name == 'GlobalSettings' and global.ClassName == 'GlobalSettings')
        assert(global:IsA('GenericSettings') and global:IsA('ServiceProvider'))
        assert(plugins.Name == 'PluginManager' and plugins:IsA('Instance'))
        assert(global.Parent == nil and plugins.Parent == nil)
        assert(game:FindService('PluginManager') == nil)
        local ok, message = pcall(function() global:GetFFlag('missing') end)
        assert(not ok and string.find(tostring(message), 'engine flag values are unavailable', 1, true))
        ok, message = pcall(function() plugins:CreatePlugin() end)
        assert(not ok and string.find(tostring(message), 'PluginSecurity is unavailable', 1, true))
        assert(not pcall(settings, true) and not pcall(PluginManager, 'x'))
    "#).exec().unwrap();
    assert_eq!(
        world.borrow().effects.get("mutations"),
        bootstrap_mutations.as_ref()
    );
}

fn collection_runtime() -> (Lua, WorldRef, super::scheduler::SchedulerRef) {
    let (lua, ctx) = setup();
    let sched=Rc::new(RefCell::new(super::scheduler::Scheduler::new()));
    super::scheduler::install(&lua,&sched).unwrap();
    lua.load(r#"
        collection=game:GetService('CollectionService'); events={}
        collection:GetInstanceAddedSignal('watch'):Connect(function(item) table.insert(events,'+'..item.Name) end)
        collection:GetInstanceRemovedSignal('watch'):Connect(function(item) table.insert(events,'-'..item.Name) end)
    "#).exec().unwrap();
    (lua,ctx,sched)
}
fn collection_pump(lua: &Lua, sched: &super::scheduler::SchedulerRef) {
    let report=super::scheduler::step(lua,sched,std::time::Instant::now(),std::time::Duration::from_millis(100));
    assert!(report.errors.is_empty(),"{:?}",report.errors);
}

#[test]
fn collection_membership_follows_local_subtrees_and_destruction() {
    let (lua,ctx,sched)=collection_runtime();
    lua.load(r#"
        folder=Instance.new('Folder'); part=Instance.new('Part',folder); part.Name='child'; part:AddTag('watch')
        assert(#collection:GetTagged('watch')==0)
        folder.Parent=game
        assert(#collection:GetTagged('watch')==1)
        local second=Instance.new('Folder',game)
        folder.Parent=second
        folder.Parent=nil
        folder.Parent=game
        game:GetService('Debris'):AddItem(folder,0)
    "#).exec().unwrap();
    services::tick(&lua,&ctx).unwrap();
    collection_pump(&lua,&sched);
    assert_eq!(lua.load("return table.concat(events,',')").eval::<String>().unwrap(),"+child,-child,+child,-child");
    lua.load("assert(#collection:GetTagged('watch')==0); folder:Destroy()").exec().unwrap();
    collection_pump(&lua,&sched);
    assert_eq!(lua.load("return #events").eval::<usize>().unwrap(),4);
}

#[test]
fn collection_membership_replays_ordered_received_changes_and_ignores_duplicates() {
    let (lua,ctx,sched)=collection_runtime();
    let delta=json!({"version":1,"delta":true,"sequence":8,"actions":[
        {"kind":"create","sequence":2,"node":{"id":"folder","class_name":"Folder","name":"folder","parent":null}},
        {"kind":"create","sequence":3,"node":{"id":"tagged","class_name":"Part","name":"received","parent":"folder","collection_tags_known":true,"collection_tags":["watch"]}},
        {"kind":"set","sequence":4,"id":"folder","fields":{"Parent":{"$ref":"game"}}},
        {"kind":"set_tags","sequence":5,"id":"tagged","tags":[]},
        {"kind":"set","sequence":6,"id":"tagged","fields":{},"tag_snapshot":{"collection_tags_known":true,"collection_tags":["watch"]}},
        {"kind":"set_property","sequence":7,"id":"folder","key":"Parent","value":null},
        {"kind":"destroy","sequence":8,"id":"folder"}
    ]});
    world::apply_delta_with_lua(&lua,&ctx,&delta,None).unwrap();
    collection_pump(&lua,&sched);
    assert_eq!(lua.load("return table.concat(events,',')").eval::<String>().unwrap(),"+received,-received,+received,-received");
    world::apply_delta_with_lua(&lua,&ctx,&delta,None).unwrap();
    collection_pump(&lua,&sched);
    assert_eq!(lua.load("return #events").eval::<usize>().unwrap(),4);
}

#[test]
fn collection_membership_is_silent_for_detached_tags_and_rejected_transactions() {
    let (lua,ctx,sched)=collection_runtime();
    world::apply_delta_with_lua(&lua,&ctx,&json!({"version":1,"delta":true,"sequence":3,"actions":[
        {"kind":"create","sequence":2,"node":{"id":"detached","class_name":"Part","name":"detached","parent":null,"collection_tags_known":true,"collection_tags":[]}},
        {"kind":"set","sequence":3,"id":"detached","fields":{},"tag_snapshot":{"collection_tags":["watch"]}}
    ]}),None).unwrap();
    let rejected=world::apply_delta_with_lua(&lua,&ctx,&json!({"version":1,"delta":true,"sequence":5,"actions":[
        {"kind":"set","sequence":4,"id":"detached","property":"Parent","value":{"$ref":"game"}},
        {"kind":"invalid","sequence":5}
    ]}),None);
    assert!(rejected.is_err());
    collection_pump(&lua,&sched);
    assert_eq!(lua.load("return #events").eval::<usize>().unwrap(),0);
    assert_eq!(ctx.borrow().sequence,3);
    world::apply_delta_with_lua(&lua,&ctx,&json!({"version":1,"delta":true,"sequence":4,"actions":[
        {"kind":"set","sequence":4,"id":"detached","property":"Parent","value":{"$ref":"game"}}
    ]}),None).unwrap();
    collection_pump(&lua,&sched);
    assert_eq!(lua.load("return table.concat(events,',')").eval::<String>().unwrap(),"+detached");
}

#[test]
fn localization_service_returns_a_translator_and_passes_text_through() {
    let snapshot = json!({"version":1,"sequence":0,"services":{"LocalizationService":"loc"},"nodes":[
        {"id":"game","class_name":"DataModel","name":"game","parent":null,"properties":{},"server_owned":true},
        {"id":"loc","class_name":"LocalizationService","name":"LocalizationService","parent":"game","properties":{},"server_owned":true}
    ]});
    let world = Rc::new(RefCell::new(World::from_snapshot(&snapshot, None).unwrap()));
    let lua = Lua::new();
    crate::rust_host::install_bindings(&lua, &world).unwrap();
    let out: (String, String) = lua
        .load(r#"
            local ls = game:GetService('LocalizationService')
            local t = ls:GetTranslatorForPlayer(nil)
            return t.ClassName, t:Translate('hello')
        "#)
        .eval()
        .unwrap();
    assert_eq!(out.0, "Translator");
    assert_eq!(out.1, "hello");
}
