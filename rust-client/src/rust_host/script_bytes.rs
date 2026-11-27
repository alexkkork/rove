//! Script bytes from this VM's admitted catalog or locally authored source.
//! No historical session payloads or placeholder bytecode are used.
use super::{lifecycle, world::WorldRef};
use mlua::{chunk::Compiler, Lua, Value};
use serde_json::Value as Json;
use sha2::{Digest, Sha256, Sha384};
use std::rc::Rc;

const MAX_BYTES: usize = 128 * 1024;

fn error(text: &str) -> mlua::Error {
    mlua::Error::RuntimeError(text.into())
}

fn decode(item: &Json, field: &str, digest: &str) -> mlua::Result<Vec<u8>> {
    let hex = item[field].as_str().ok_or_else(|| error("script bytecode is unavailable"))?;
    if hex.is_empty() || hex.len() % 2 != 0 || hex.len() > MAX_BYTES * 2 {
        return Err(error("invalid script bytecode size"));
    }
    let bytes = hex.as_bytes().chunks_exact(2).map(|p| {
        let a = (p[0] as char).to_digit(16).ok_or_else(|| error("invalid script bytecode encoding"))?;
        let b = (p[1] as char).to_digit(16).ok_or_else(|| error("invalid script bytecode encoding"))?;
        Ok((a * 16 + b) as u8)
    }).collect::<mlua::Result<Vec<_>>>()?;
    if let Some(expected) = item.get(digest) {
        if expected.as_str() != Some(format!("{:x}", Sha256::digest(&bytes)).as_str()) {
            return Err(error("script bytecode digest mismatch"));
        }
    }
    Ok(bytes)
}

fn item_bytes(item: &Json, normalized: bool) -> mlua::Result<Vec<u8>> {
    // Pinned builtins retain the compiler/version represented by their catalog.
    if item.get("bytecode_provenance").is_some() {
        return decode(item, "bytecode_hex", "bytecode_sha256");
    }
    if let Some(source) = item["source"].as_str() {
        if source.len() > MAX_BYTES { return Err(error("script source exceeds limit")); }
        return Compiler::new().set_debug_level(2).compile(source.as_bytes());
    }
    if !normalized && item.get("original_bytecode_hex").is_some() {
        return decode(item, "original_bytecode_hex", "original_sha256");
    }
    decode(item, "bytecode_hex", "normalized_sha256")
}

pub(super) fn bytes(lua: &Lua, ctx: &WorldRef, value: Value, normalized: bool) -> mlua::Result<Vec<u8>> {
    let item = lifecycle::script_code(lua, ctx, value)?;
    super::integrity::check_load_thread(lua)?;
    item_bytes(&item, normalized)
}

pub(super) fn install(lua: &Lua, ctx: &WorldRef) -> mlua::Result<()> {
    for (name, hash) in [("getscriptbytecode", false), ("getscripthash", true)] {
        let weak = Rc::downgrade(ctx);
        lua.globals().set(name, lua.create_function(move |lua, value: Value| {
            let ctx = weak.upgrade().ok_or_else(|| error("world has been released"))?;
            // `getscriptbytecode` exposes the admitted, VM-ready form. The
            // hash API intentionally uses the untouched session payload.
            let code = match bytes(lua, &ctx, value, !hash) {
                Err(mlua::Error::RuntimeError(message)) if !hash && message == "code_unavailable" => {
                    return Ok(Value::Nil);
                }
                result => result?,
            };
            let string = if hash { lua.create_string(format!("{:x}", Sha384::digest(&code)))? }
                else { lua.create_string(code)? };
            Ok(Value::String(string))
        })?)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use serde_json::json;

    #[test]
    fn admitted_bytecode_selection_preserves_original_and_checks_digest() {
        // This catalog entry is a local fixture representing an entry that
        // has already passed admission. Its normalized payload is executable
        // bytecode produced by the official Luau compiler; the original field
        // remains a separate, deliberately different transport payload.
        let normalized = Compiler::new().set_debug_level(2).compile(b"return 42").unwrap();
        let original = [0x52, 0x53, 0x42, 0x31, 0x00, 0xfe];
        let mut item = json!({
            "bytecode_hex": normalized.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            "original_bytecode_hex": original.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            "signature_verified": true,
            "normalized_sha256": format!("{:x}", Sha256::digest(&normalized)),
            "original_sha256": format!("{:x}", Sha256::digest(original)),
        });
        assert_eq!(item_bytes(&item, false).unwrap(), original);
        let admitted = item_bytes(&item, true).unwrap();
        assert_eq!(admitted, normalized);

        // Validate the selected normalized bytes with the official VM using
        // only this test-owned constant return chunk.
        let lua = Lua::new();
        assert_eq!(lua.load(&admitted).set_mode(mlua::chunk::ChunkMode::Binary).eval::<i64>().unwrap(), 42);
        assert_ne!(Sha384::digest(&item_bytes(&item, false).unwrap()), Sha384::digest(&admitted));

        item["original_sha256"] = json!("wrong");
        assert!(item_bytes(&item, false).is_err());
        item["original_sha256"] = json!(format!("{:x}", Sha256::digest(original)));
        item["normalized_sha256"] = json!("wrong");
        assert!(item_bytes(&item, true).is_err());
        assert!(item_bytes(&json!({"reason":"code_unavailable"}),false).is_err());
    }

    #[test]
    fn authored_script_bytes_execute_and_hash_tracks_source_without_admitting_server_source() {
        let lua = Lua::new();
        let ctx = Rc::new(RefCell::new(super::super::world::World::from_snapshot(&json!({
            "version":1,"sequence":0,"nodes":[
                {"id":"game","class_name":"DataModel","name":"game"},
                {"id":"remote","class_name":"LocalScript","name":"Unavailable","parent":"game",
                 "server_owned":true,"properties":{"Source":"return 'not admitted'"}}
            ]}),None).unwrap()));
        super::super::install_bindings(&lua,&ctx).unwrap();
        let code: mlua::String = lua.load(r#"
            local script=Instance.new('LocalScript')
            script.Source='return 42'
            local before=getscriptbytecode(script)
            local hash=getscripthash(script)
            assert(#hash==96 and hash==getscripthash(script))
            script.Source='return 43'
            assert(getscriptbytecode(script)~=before and getscripthash(script)~=hash)
            assert(not pcall(getscriptbytecode,game))
            -- A received Source is usable even on a server-owned node.
            assert(getscriptbytecode(game.Unavailable)~=nil)
            assert(getscriptbytecode(Instance.new('LocalScript'))==nil)
            local ok, fn, message=pcall(loadstring,before)
            assert(ok and fn==nil and type(message)=='string')
            local invalid, reason=loadstring(string.char(255,0,128))
            assert(invalid==nil and type(reason)=='string')
            local authored=loadstring('return function() return 7 end')
            assert(isexecutorclosure(authored) and isexecutorclosure(authored()))
            writefile('binary-source.luau', 'return "'..string.char(255)..'"')
            assert(loadfile('binary-source.luau')()==string.char(255))
            return before
        "#).eval().unwrap();
        assert_eq!(lua.load(code.as_bytes().as_ref()).set_mode(mlua::chunk::ChunkMode::Binary).eval::<i32>().unwrap(),42);
    }
}
