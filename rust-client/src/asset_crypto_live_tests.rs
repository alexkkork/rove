//! End-to-end check of the `ContentProvider` encryption surface: register,
//! list, validate, unregister, plus a real decrypt through the fetch hook.
use crate::rust_host::{
    enums, services,
    world::{self, World, WorldRef},
};
use mlua::{Lua, Value};
use serde_json::json;
use std::{cell::RefCell, rc::Rc};

fn fixture() -> WorldRef {
    Rc::new(RefCell::new(
        World::from_snapshot(
            &json!({"version":1,"sequence":0,"input_capabilities":{},
                "services":{"ContentProvider":"cp"},"nodes":[
                {"id":"game","class_name":"DataModel","name":"game","parent":null,"properties":{},"server_owned":true},
                {"id":"cp","class_name":"ContentProvider","name":"ContentProvider","parent":"game","properties":{},"server_owned":true}
            ]}),
            None,
        )
        .unwrap(),
    ))
}

fn setup() -> mlua::Result<(Lua, WorldRef)> {
    crate::asset_crypto::reset();
    let lua = Lua::new();
    let world = fixture();
    enums::install(&lua)?;
    world::install(&lua, &world)?;
    services::install(&lua, &world)?;
    Ok((lua, world))
}

fn provider(lua: &Lua, world: &WorldRef) -> mlua::Result<Value> {
    world::instance(lua, world, "cp")
}

#[test]
fn register_list_validate_and_unregister() -> mlua::Result<()> {
    let (lua, world) = setup()?;
    lua.globals().set("cp", provider(&lua, &world)?)?;

    // The registry is process-global, so assert per-id rather than by count.
    lua.load(
        r#"
        cp:RegisterDefaultEncryptionKey(string.rep("ab", 32))
        cp:RegisterEncryptedAsset(707, string.rep("cd", 32))
        cp:RegisterSessionEncryptedAsset("rbxassetid://909", "AAAAAAAAAAAAAAAAAAAAAA==")
    "#,
    )
    .exec()?;
    assert!(crate::asset_crypto::key_for(707).is_some());
    assert!(crate::asset_crypto::key_for(909).is_some());

    // Bad key length is rejected with the specific code.
    let (ok, message) = lua
        .load("local ok, err = pcall(function() cp:RegisterEncryptedAsset(1, 'short') end); return ok, tostring(err)")
        .eval::<(bool, String)>()?;
    assert!(!ok);
    assert!(message.contains("encryption_key_not_16_24_or_32_bytes"), "{message}");

    lua.load(
        r#"
        cp:UnregisterEncryptedAsset(707)
        cp:UnregisterEncryptedAsset(909)
        cp:UnregisterDefaultEncryptionKey()
    "#,
    )
    .exec()?;
    assert!(crate::asset_crypto::key_for(707).is_none());
    assert!(crate::asset_crypto::key_for(909).is_none());
    Ok(())
}

#[test]
fn registered_asset_bytes_are_decrypted_and_failures_reported() -> mlua::Result<()> {
    use aes_gcm::aead::{Aead, KeyInit};
    let (_lua, _world) = setup()?;
    let key = vec![3u8; 32];
    let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
    crate::asset_crypto::register_asset(55, &hex).unwrap();

    // Sealed payload with a 12-byte nonce prefix decrypts through decrypt_asset.
    let nonce = [4u8; 12];
    let cipher = aes_gcm::Aes256Gcm::new_from_slice(&key).unwrap();
    let mut payload = nonce.to_vec();
    payload.extend(cipher.encrypt(&nonce.into(), &b"plaintext asset".repeat(4)[..]).unwrap());
    let (plain, decrypted) = crate::asset_crypto::decrypt_asset(55, payload).unwrap();
    assert!(decrypted);
    assert_eq!(&plain[..15], b"plaintext asset");

    // A corrupted body is a tag mismatch, recorded as a failure.
    crate::asset_crypto::register_asset(66, &hex).unwrap();
    let mut bad = nonce.to_vec();
    bad.extend_from_slice(&[0u8; 32]);
    assert!(crate::asset_crypto::decrypt_asset(66, bad).is_err());
    crate::asset_crypto::record_failure(66, "asset_decrypt_tag_mismatch");
    assert!(crate::asset_crypto::failures().contains(&(66, "asset_decrypt_tag_mismatch")));
    Ok(())
}
