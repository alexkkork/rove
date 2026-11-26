//! Wire names that project onto public engine properties.
use serde_json::{json, Value};

pub(crate) fn from_wire(class: &str, key: &str, value: &Value) -> Result<Option<Value>, String> {
    match (class, key) {
        (_, "archivable") => value.as_bool().map(|v| Some(json!(v)))
            .ok_or_else(|| "received archivable must be a boolean".into()),
        (_, "Color3uint8") if is_part(class) => Ok(Some(color3uint8(value)?)),
        (_, "CollisionGroupReplicate") if is_part(class) => Ok(Some(collision_group(value)?)),
        _ => Ok(None),
    }
}

/// Decode the wire `Color3uint8` into a tagged Color3. The canonical encoding
/// is `{$color3:[r,g,b]}` with 0..1 floats, but the compact/ISR paths can hand
/// over a bare triple or a 0..255 byte triple. Normalise all of them.
fn color3uint8(value: &Value) -> Result<Value, String> {
    // Null means the stream omitted the color; keep it null and let the
    // constructor default resolve it at read time.
    if value.is_null() {
        return Ok(Value::Null);
    }
    let err = format!("received Color3uint8 must be a tagged Color3 (got {value})");
    let body = if let Some(tagged) = value.get("$color3") {
        tagged
    } else if value.is_array() || value.get("r").is_some() {
        value
    } else {
        return Err(err.clone());
    };
    let raw: Vec<&Value> = match body {
        Value::Array(items) => items.iter().collect(),
        Value::Object(map) => {
            let mut out = Vec::with_capacity(3);
            for key in ["r", "g", "b"] {
                out.push(map.get(key).ok_or_else(|| err.clone())?);
            }
            out
        }
        _ => return Err(err.into()),
    };
    if raw.len() != 3 {
        return Err(err.clone());
    }
    let mut out = [0f64; 3];
    let mut byte_scale = false;
    for (i, item) in raw.iter().enumerate() {
        let number = item.as_f64().ok_or_else(|| err.clone())?;
        if !number.is_finite() {
            return Err("received Color3uint8 component is out of range".into());
        }
        if number > 1.0 {
            byte_scale = true;
        }
        out[i] = number;
    }
    let limit = if byte_scale { 255.0 } else { 1.0 };
    for number in &mut out {
        if !(0.0..=limit).contains(number) {
            return Err("received Color3uint8 component is out of range".into());
        }
        if byte_scale {
            *number /= 255.0;
        }
    }
    Ok(json!({"$color3": [out[0], out[1], out[2]]}))
}

fn collision_group(value: &Value) -> Result<Value, String> {
    let bytes = if let Some(text) = value.as_str() {
        text.as_bytes().to_vec()
    } else if let Some(hex) = value.get("$bytes_hex").and_then(Value::as_str) {
        if hex.len() % 2 != 0 || hex.len() > 2 * 1024 * 1024 {
            return Err("invalid CollisionGroupReplicate byte-string length".into());
        }
        let mut bytes = Vec::with_capacity(hex.len() / 2);
        for pair in hex.as_bytes().chunks_exact(2) {
            let digit = |b: u8| match b {
                b'0'..=b'9' => Some(b - b'0'),
                b'a'..=b'f' => Some(b - b'a' + 10),
                b'A'..=b'F' => Some(b - b'A' + 10),
                _ => None,
            };
            let hi = digit(pair[0]).ok_or("invalid CollisionGroupReplicate byte-string hex")?;
            let lo = digit(pair[1]).ok_or("invalid CollisionGroupReplicate byte-string hex")?;
            bytes.push((hi << 4) | lo);
        }
        bytes
    } else {
        return Err(
            "received CollisionGroupReplicate must be a string or tagged byte-string".into(),
        );
    };
    if bytes.is_empty() {
        return Err("received CollisionGroupReplicate is missing its group ID".into());
    }
    if bytes.len() == 1 {
        return Ok(json!("Default"));
    }
    let name = &bytes[1..];
    Ok(match std::str::from_utf8(name) {
        Ok(name) => json!(name),
        Err(_) => {
            json!({"$bytes_hex": name.iter().map(|b| format!("{b:02x}")).collect::<String>()})
        }
    })
}

fn is_part(class: &str) -> bool {
    matches!(
        class,
        "BasePart"
            | "Part"
            | "MeshPart"
            | "UnionOperation"
            | "WedgePart"
            | "CornerWedgePart"
            | "TrussPart"
            | "Seat"
            | "VehicleSeat"
            | "SpawnLocation"
    ) || super::world::class_chain(class)
        .iter()
        .any(|base| base == "BasePart")
}
pub(crate) fn canonical_key(class: &str, wire_key: &str) -> Option<&'static str> {
    if wire_key == "archivable" { return Some("Archivable"); }
    let key = match wire_key {
        "Color3uint8" => "Color",
        "CollisionGroupReplicate" => "CollisionGroup",
        _ => return None,
    };
    is_part(class).then_some(key)
}
pub(crate) fn wire_key(class: &str, public_key: &str) -> Option<&'static str> {
    if public_key == "Archivable" { return Some("archivable"); }
    let key = match public_key {
        "Color" => "Color3uint8",
        "CollisionGroup" => "CollisionGroupReplicate",
        _ => return None,
    };
    is_part(class).then_some(key)
}

pub(crate) fn is_lightweight_asset(key: &str) -> bool {
    matches!(
        key,
        "ModelMeshData"
            | "ModelMeshCFrame"
            | "ModelMeshSize"
            | "MeshId"
            | "MeshContent"
            | "TextureID"
            | "TextureId"
            | "Texture"
            | "TextureContent"
            | "TexturePack"
            | "Image"
            | "ImageContent"
            | "ColorMap"
            | "ColorMapContent"
            | "NormalMap"
            | "NormalMapContent"
            | "MetalnessMap"
            | "MetalnessMapContent"
            | "RoughnessMap"
            | "RoughnessMapContent"
            | "SoundId"
            | "AudioContent"
            | "Video"
            | "VideoContent"
            | "AnimationId"
            | "AnimationContent"
    )
}
