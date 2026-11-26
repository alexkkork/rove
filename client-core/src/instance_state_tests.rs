use super::*;
use crate::{
    schema::{ClassDesc, PropertyDesc},
    session::{IngestOutcome, SessionCore, SessionError},
    world::WorldStore,
};

fn schema() -> Schema {
    Schema {
        version: 3,
        flags: 0,
        enums: vec![],
        classes: vec![ClassDesc {
            id: 0,
            name: "Part".into(),
            properties: vec![],
            events: vec![],
            isr_properties: vec![0, 1, 2],
            unknown_u16: 0,
        }],
        properties: [
            ("PhysicsRepRootPart", 28),
            ("PhysicsRepRootRef", 69),
            ("ReplicationPV", 57),
        ]
        .into_iter()
        .enumerate()
        .map(|(id, (name, type_id))| PropertyDesc {
            id,
            class: 0,
            name: name.into(),
            type_id,
            enum_id: None,
            lr_type: None,
        })
        .collect(),
        events: vec![],
        strings: vec![],
        names: vec![],
        lr_types: vec![],
        parent_property_id: 3,
        decoded_bytes: 0,
    }
}
fn hex(text: &str) -> Vec<u8> {
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}
fn packet(timestamp: u32, lod: u8, records: &[(u32, u64, Vec<u8>)]) -> Vec<u8> {
    let mut out = vec![161];
    out.extend(timestamp.to_le_bytes());
    out.extend(1u32.to_le_bytes());
    out.extend(i32::MAX.to_le_bytes());
    out.extend((records.len() as u16).to_le_bytes());
    out.extend([lod, 0]);
    for (id, mask, body) in records {
        out.push(1);
        out.extend(id.to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out.extend(3u16.to_le_bytes());
        out.extend(mask.to_le_bytes());
        out.extend((body.len() as u16).to_le_bytes());
        out.extend(body);
    }
    out
}
fn world() -> WorldStore {
    WorldStore::from_snapshot(&json!({"version":1,"sequence":0,"nodes":[
        {"id":"1:42","class_name":"Part","created_sequence":1,"properties":{}},
        {"id":"1:43","class_name":"Part","created_sequence":1,"properties":{}}
    ]}))
    .unwrap()
}

#[test]
fn captured_pv_fields_match_reference_for_all_four_lods() {
    let fixtures: Value =
        serde_json::from_str(include_str!("../tests/fixtures/isr-pv.json")).unwrap();
    for fixture in fixtures["cases"].as_array().unwrap() {
        let lod = fixture["lod"].as_u64().unwrap() as u8;
        let packet = decode_packet(
            &packet(
                100,
                lod,
                &[(42, 4, hex(fixture["body_hex"].as_str().unwrap()))],
            ),
            &schema(),
            true,
        )
        .unwrap();
        let value = &packet.records[0].fields["ReplicationPV"];
        for (actual, expected) in [
            ("position", "position"),
            ("rotation", "rotation"),
            ("decoded_velocities", "velocities"),
        ] {
            assert_eq!(value[actual], fixture[expected], "LOD {lod}: {actual}");
        }
    }
}

#[test]
fn received_velocities_project_without_overwriting_newer_explicit_properties() {
    use crate::runtime_snapshot::{
        normalize_node_values, project_node_origin, project_node_property, project_node_revision,
    };
    let mut node = json!({"id":"p","class_name":"Part","properties":{
        "ReplicationPV":{"position":[1,2,3],"rotation":[1,0,0,0,1,0,0,0,1],"decoded_velocities":[[4,5,6],[7,8,9]]},
        "Velocity":{"$vector3":[90,80,70]}
    },"origins":{"Velocity":"received"},"property_revisions":{"ReplicationPV":10,"Velocity":11}});
    let store =
        WorldStore::from_snapshot(&json!({"version":1,"sequence":11,"nodes":[node]})).unwrap();
    let handle = store.view().read_only_node("p").unwrap();
    normalize_node_values(&mut node, None).unwrap();
    for key in [
        "AssemblyLinearVelocity",
        "Velocity",
        "AssemblyAngularVelocity",
        "RotVelocity",
    ] {
        assert_eq!(
            project_node_property(&handle, None, key).unwrap(),
            Some(node["properties"][key].clone())
        );
        assert_eq!(
            project_node_revision(&handle, None, key).unwrap(),
            Some(node["property_revisions"][key].clone())
        );
        assert_eq!(
            project_node_origin(&handle, None, key).unwrap(),
            Some(node["origins"][key].clone())
        );
    }
    assert_eq!(
        node["properties"]["Velocity"],
        json!({"$vector3":[90,80,70]})
    );
    assert_eq!(
        node["properties"]["AssemblyLinearVelocity"],
        json!({"$vector3":[4,5,6]})
    );
}

#[test]
fn all_truncations_bad_body_lengths_and_trailing_bytes_reject_the_whole_packet() {
    let good = packet(1, 0, &[(42, 4, vec![0xe1]), (43, 4, vec![0xe1])]);
    for length in 0..good.len() {
        assert!(
            decode_packet(&good[..length], &schema(), true).is_err(),
            "offset {length}"
        );
    }
    let mut trailing = good;
    trailing.push(0);
    assert!(decode_packet(&trailing, &schema(), true).is_ok());
    // An empty body is short; a complete ref plus a trailing pad decodes.
    assert!(decode_packet(&packet(1, 0, &[(42, 4, vec![])]), &schema(), true).is_err());
    assert!(decode_packet(&packet(1, 0, &[(42, 4, vec![0xe1, 0])]), &schema(), true).is_ok());
    assert!(decode_packet(&packet(1, 4, &[(42, 4, vec![0xe1])]), &schema(), true).is_err());
    assert!(decode_packet(&packet(1, 0, &[(42, 8, vec![])]), &schema(), true).is_err());
}

#[test]
fn references_allow_null_and_format_requires_received_feature_configuration() {
    let refs = packet(1, 0, &[(42, 3, vec![1, 17, 0, 0, 0, 0])]);
    let result = decode_packet(&refs, &schema(), false).unwrap();
    assert_eq!(
        result.records[0].fields["PhysicsRepRootPart"],
        json!({"$ref":"1:17"})
    );
    assert_eq!(result.records[0].fields["PhysicsRepRootRef"], Value::Null);
    assert!(decode_packet(&packet(1, 0, &[(42, 4, vec![0xe1])]), &schema(), false).is_err());
    for selector in 25..=31 {
        assert!(decode_packet(
            &packet(1, 0, &[(42, 4, vec![0xe0 | selector])]),
            &schema(),
            true
        )
        .is_err());
    }
}

#[test]
fn every_compact_orientation_is_a_right_handed_basis() {
    for selector in 1..=24 {
        let decoded = decode_packet(
            &packet(1, 0, &[(42, 4, vec![0xe0 | selector])]),
            &schema(),
            true,
        )
        .unwrap();
        let matrix = decoded.records[0].fields["ReplicationPV"]["rotation"]
            .as_array()
            .unwrap();
        let m: Vec<f64> = matrix.iter().map(|v| v.as_f64().unwrap()).collect();
        for a in 0..3 {
            for b in 0..3 {
                let dot = (0..3).map(|i| m[3 * i + a] * m[3 * i + b]).sum::<f64>();
                assert_eq!(dot, if a == b { 1. } else { 0. });
            }
        }
        assert_eq!(
            m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6])
                + m[2] * (m[3] * m[7] - m[4] * m[6]),
            1.
        );
    }
}

#[test]
fn timestamps_are_per_instance_wrap_and_do_not_apply_to_recreated_instances() {
    let mut world = world();
    let mut receiver = Receiver::default();
    receiver.configure(true);
    let p = receiver
        .prepare(
            &packet(0xfffffff0, 0, &[(42, 4, vec![0xe1])]),
            &schema(),
            &world.view(),
        )
        .unwrap();
    receiver.committed(p);
    let p = receiver
        .prepare(
            &packet(5, 0, &[(42, 4, vec![0xe1])]),
            &schema(),
            &world.view(),
        )
        .unwrap();
    receiver.committed(p);
    let p = receiver
        .prepare(
            &packet(0xfffffff1, 0, &[(42, 4, vec![0xe1]), (43, 4, vec![0xe1])]),
            &schema(),
            &world.view(),
        )
        .unwrap();
    assert_eq!(p.records.len(), 1);
    assert_eq!(p.records[0].id.as_deref(), Some("1:43"));
    world.apply_delta(&json!({"version":1,"delta":true,"sequence":2,"actions":[{"kind":"remove","id":"1:42","sequence":2}]})).unwrap();
    world.apply_delta(&json!({"version":1,"delta":true,"sequence":3,"actions":[{"kind":"create","id":"1:42","sequence":3,"node":{"id":"1:42","class_name":"Part","properties":{}}}]})).unwrap();
    assert_eq!(
        receiver
            .prepare(
                &packet(0xfffffff2, 0, &[(42, 4, vec![0xe1])]),
                &schema(),
                &world.view()
            )
            .unwrap()
            .records
            .len(),
        1
    );
}

fn session(limit: usize) -> SessionCore {
    let mut session = SessionCore::with_schema(schema(), limit, 1024 * 1024).unwrap();
    session.set_instance_state_legacy_pv(true);
    session.ingest(&[129, 0, 0, 0, 0, 0, 0, 0, 1, 0]).unwrap();
    for id in [42u32, 43] {
        let mut create = vec![131, 2, 1];
        create.extend(id.to_le_bytes());
        create.extend([0, 0, 255, 255, 0, 0]);
        session.ingest(&create).unwrap();
    }
    session
}

#[test]
fn movement_receipts_share_world_sequence_with_ordinary_replication() {
    let mut s = session(16);
    let reader = s.retain(0).unwrap();
    let old = s.world_sequence().unwrap();
    let result = s
        .ingest_instance_state(&packet(1, 0, &[(42, 4, vec![0xe1]), (43, 4, vec![0xe2])]))
        .unwrap()
        .unwrap();
    assert!(matches!(result, IngestOutcome::ReplicationCommitted { .. }));
    assert_eq!(s.world_sequence(), Some(old + 2));
    let view = s.world_view().unwrap();
    assert_ne!(
        view.get_property("1:42", "ReplicationPV"),
        view.get_property("1:43", "ReplicationPV")
    );
    let mut remove = vec![131, 1, 1];
    remove.extend(42u32.to_le_bytes());
    remove.push(0);
    s.ingest(&remove).unwrap();
    assert_eq!(s.world_sequence(), Some(old + 3));
    assert!(!s.world_view().unwrap().contains("1:42"));
    let rows = s.read_after(reader, 0, 16).unwrap();
    assert_eq!(rows[3].value.packet_kind, 161);
    assert!(rows[3].value.property_changes.is_empty());
}

#[test]
fn journal_backpressure_does_not_advance_the_world_or_timestamp_fence() {
    let mut s = session(3);
    let reader = s.retain(0).unwrap();
    let before = s.snapshot().unwrap();
    let payload = packet(100, 0, &[(42, 4, vec![0xe1])]);
    assert!(matches!(
        s.ingest_instance_state(&payload),
        Err(SessionError::Journal(_))
    ));
    assert_eq!(s.snapshot().unwrap(), before);
    assert_eq!(s.instance_state_report()["records_applied"], 0);
    s.release(reader).unwrap();
    assert!(s
        .ingest_instance_state(&packet(99, 0, &[(42, 4, vec![0xe1])]))
        .unwrap()
        .is_some());
    assert_eq!(s.instance_state_report()["records_applied"], 1);
}

#[test]
fn invalid_unknown_and_wrong_class_records_never_create_or_partially_change_instances() {
    let mut s = session(16);
    let before = s.snapshot().unwrap();
    let mut broken = packet(1, 0, &[(42, 4, vec![0xe1]), (43, 4, vec![0xe1])]);
    broken.pop();
    assert!(s.ingest_instance_state(&broken).unwrap().is_none());
    assert_eq!(s.snapshot().unwrap(), before);
    assert_eq!(s.instance_state_report()["packets_rejected"], 1);
    assert!(s
        .ingest_instance_state(&packet(1, 0, &[(99, 4, vec![0xe1])]))
        .unwrap()
        .is_none());
    assert!(!s.world_view().unwrap().contains("1:99"));
    let wrong_class = WorldStore::from_snapshot(&json!({"version":1,"sequence":0,"nodes":[
        {"id":"1:42","class_name":"Model","properties":{}}
    ]}))
    .unwrap();
    let mut receiver = Receiver::default();
    receiver.configure(true);
    let prepared = receiver
        .prepare(
            &packet(1, 0, &[(42, 4, vec![0xe1])]),
            &schema(),
            &wrong_class.view(),
        )
        .unwrap();
    assert!(prepared.records.is_empty());
    assert_eq!(receiver.report()["unresolved_records"], 1);
}

#[test]
fn received_motion_revisions_select_the_latest_pose_and_preserve_export_parity() {
    use crate::runtime_snapshot::{normalize_node_values, project_node_property};
    let pv = |x: i32, speed: i32| json!({"position":[x,0,0],"rotation":[1,0,0,0,1,0,0,0,1],"decoded_velocities":[[speed,0,0],[0,0,0]]});
    let cf = |x: i32| json!({"$cframe":{"position":[x,0,0],"rotation":[1,0,0,0,1,0,0,0,1]}});
    let mut store = WorldStore::from_snapshot(&json!({"version":1,"sequence":1,"nodes":[{
        "id":"p","class_name":"Part","properties":{"CFrame":cf(0),"ReplicationPV":pv(0,0)},
        "property_revisions":{"CFrame":1,"ReplicationPV":1}
    }]})).unwrap();
    let old=store.view();
    for (index,(fields,x,speed)) in [
        (json!({"ReplicationPV":pv(10,3)}),10,3),
        (json!({"CFrame":cf(20),"AssemblyLinearVelocity":{"$vector3":[7,0,0]}}),20,7),
        (json!({"ReplicationPV":pv(30,5)}),30,5),
        (json!({"ReplicationPV":pv(40,6),"CFrame":cf(50),"AssemblyLinearVelocity":{"$vector3":[8,0,0]}}),50,8),
    ].into_iter().enumerate() {
        let seq=index as u64+2;
        let delta=json!({"version":1,"delta":true,"sequence":seq,"actions":[{"kind":"set","id":"p","sequence":seq,"fields":fields}]});
        store.apply_delta(&delta).unwrap();
        let node=store.view().read_only_node("p").unwrap();
        let mut exported=store.to_snapshot()["nodes"][0].clone();
        normalize_node_values(&mut exported,None).unwrap();
        for key in ["CFrame","Position","AssemblyLinearVelocity"] {
            assert_eq!(project_node_property(&node,None,key).unwrap(),Some(exported["properties"][key].clone()),"step={index} key={key}");
        }
        assert_eq!(exported["properties"]["Position"],json!({"$vector3":[x,0,0]}));
        assert_eq!(exported["properties"]["AssemblyLinearVelocity"],json!({"$vector3":[speed,0,0]}));
        let before=store.to_snapshot();
        store.apply_delta(&delta).unwrap();
        assert_eq!(store.to_snapshot(),before,"repeated receipts cannot change revisions");
    }
    let old_node=old.read_only_node("p").unwrap();
    assert_eq!(project_node_property(&old_node,None,"Position").unwrap(),Some(json!({"$vector3":[0,0,0]})));
    assert_eq!(old_node.metadata_field("property_revisions","ReplicationPV"),Some(json!(1)));
}

#[test]
fn invalid_motion_batch_rolls_back_values_and_revision_stamps() {
    let mut store=WorldStore::from_snapshot(&json!({"version":1,"sequence":0,"nodes":[{"id":"p","class_name":"Part","properties":{}}]})).unwrap();
    let before=store.to_snapshot();
    let delta=json!({"version":1,"delta":true,"sequence":2,"actions":[
        {"kind":"set","id":"p","sequence":1,"fields":{"CFrame":{"$cframe":{"position":[1,0,0],"rotation":[1,0,0,0,1,0,0,0,1]}}}},
        {"kind":"unsupported","id":"p","sequence":2}
    ]});
    assert!(store.apply_delta(&delta).is_err());
    assert_eq!(store.to_snapshot(),before);
}

#[test]
fn motion_revision_tracking_is_bounded_and_accepts_absent_or_null_metadata() {
    for revisions in [None,Some(Value::Null)] {
        let mut node=json!({"id":"p","class_name":"Part","properties":{}});
        if let Some(value)=revisions { node["property_revisions"]=value; }
        let mut store=WorldStore::from_snapshot(&json!({"version":1,"sequence":0,"nodes":[node]})).unwrap();
        for sequence in 1..=100 {
            store.apply_delta(&json!({"version":1,"delta":true,"sequence":sequence,"actions":[
                {"kind":"set","id":"p","sequence":sequence,"fields":{
                    "CFrame":{"$cframe":{"position":[sequence,0,0],"rotation":[1,0,0,0,1,0,0,0,1]}},"Transparency":0.5
                }}
            ]})).unwrap();
        }
        assert_eq!(store.to_snapshot()["nodes"][0]["property_revisions"],json!({"CFrame":100}));
    }
}

#[test]
fn mass_dependency_revisions_are_bounded_and_preserve_old_views() {
    for metadata in [None, Some(Value::Null)] {
        let mut node = json!({"id":"p","class_name":"Part","properties":{}});
        if let Some(value) = metadata {node["property_revisions"] = value;}
        let mut store = WorldStore::from_snapshot(&json!({"version":1,"sequence":0,"nodes":[node]})).unwrap();
        let old = store.view();
        for sequence in 1..=100 {
            let delta=json!({"version":1,"delta":true,"sequence":sequence,"actions":[
                {"kind":"set","id":"p","sequence":sequence,"fields":{
                    "Mass":8,"Size":{"$vector3":[2,2,2]},"size":{"$vector3":[2,2,2]},
                    "Shape":1,"Material":256,"MaterialVariant":"",
                    "CustomPhysicalProperties":null,"CurrentPhysicalProperties":[1,0.3,0.5,1,1],"Transparency":0.5
                }}]});
            store.apply_delta(&delta).unwrap();
            let before=store.to_snapshot();
            store.apply_delta(&delta).unwrap();
            assert_eq!(store.to_snapshot(),before);
        }
        let exported=store.to_snapshot();
        let revisions=exported["nodes"][0]["property_revisions"].as_object().unwrap();
        assert_eq!(revisions.len(),8);
        let current=store.view().read_only_node("p").unwrap();
        for (key,value) in revisions {
            assert_eq!(value,&json!(100));
            assert_eq!(current.metadata_field("property_revisions",key),Some(value.clone()));
            assert_eq!(old.read_only_node("p").unwrap().metadata_field("property_revisions",key),None);
        }
    }
}

#[test]
fn invalid_mass_batch_rolls_back_dependency_revisions() {
    let mut store=WorldStore::from_snapshot(&json!({"version":1,"sequence":0,"nodes":[{"id":"p","class_name":"Part","properties":{"Mass":8}}]})).unwrap();
    let before=store.to_snapshot();
    assert!(store.apply_delta(&json!({"version":1,"delta":true,"sequence":2,"actions":[
        {"kind":"set","id":"p","sequence":1,"fields":{"Size":{"$vector3":[4,2,2]},"Mass":16}},
        {"kind":"unsupported","id":"p","sequence":2}
    ]})).is_err());
    assert_eq!(store.to_snapshot(),before);
}
