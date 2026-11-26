//! Received ISR updates (app 4, channel 5). Nothing here sends movement.
use crate::{
    replication::{compact_position, finite_f32, read_key},
    schema::Schema,
    wire::{Reader, WireError},
    world::WorldView,
};
use serde_json::{json, Map, Value};
use std::{collections::HashMap, sync::Arc};

const MAX_RECORDS: usize = 4096;
const MAX_PACKET_BYTES: usize = 1024 * 1024;
const MAX_TRACKED: usize = 100_000;

#[derive(Debug)]
pub struct Record {
    pub id: Option<String>,
    pub class_index: usize,
    pub fields: Map<String, Value>,
    pub lod: u8,
    pub time_delta: u8,
}
#[derive(Debug)]
pub struct Packet {
    pub timestamp: u32,
    pub header_value: i32,
    pub group_count: u32,
    pub records: Vec<Record>,
}

fn reference(r: &mut Reader<'_>) -> Result<Option<String>, WireError> {
    Ok(read_key(r)?.map(|(scope, index)| format!("{scope}:{index}")))
}

/// Validate every record and its exact body boundary before publishing values.
pub fn decode_packet(
    payload: &[u8],
    schema: &Schema,
    legacy_pv: bool,
) -> Result<Packet, WireError> {
    decode_packet_mode(payload, schema, legacy_pv, false)
}

/// Tolerant decode used by the live receiver: records that fail part-way are
/// dropped individually instead of discarding the whole packet.
pub fn decode_packet_lenient(
    payload: &[u8],
    schema: &Schema,
    legacy_pv: bool,
) -> Result<Packet, WireError> {
    decode_packet_mode(payload, schema, legacy_pv, true)
}

fn decode_packet_mode(
    payload: &[u8],
    schema: &Schema,
    legacy_pv: bool,
    lenient: bool,
) -> Result<Packet, WireError> {
    if payload.len() > MAX_PACKET_BYTES {
        return Err(WireError::new("ISR packet exceeds limit"));
    }
    let mut r = Reader::new(payload);
    if r.u8()? != 161 {
        return Err(WireError::new("Not an ISR packet"));
    }
    let timestamp = r.u32()?;
    let group_count = r.u32()?;
    let header_value = r.i32()?;
    if group_count as usize > MAX_RECORDS {
        return Err(WireError::new("ISR group count exceeds limit"));
    }
    let mut records = Vec::new();
    let mut records_bad = false;
    for _ in 0..group_count {
        // Live packets may end mid-group; keep what decodes.
        let header = (|| -> Result<(u16, u8, u8), WireError> {
            Ok((r.u16()?, r.u8()?, r.u8()?))
        })();
        let (count_raw, lod, time_delta) = match header {
            Ok(values) => values,
            Err(error) if lenient && !records.is_empty() => {
                let _ = error;
                break;
            }
            Err(error) => return Err(error),
        };
        let count = usize::from(count_raw);
        if lod > 3 || records.len() + count > MAX_RECORDS {
            if lenient {
                break;
            }
            return Err(WireError::new("Invalid ISR LOD or record count"));
        }
        for _ in 0..count {
            // A single unmatched record leaves the reader at an unknown
            // boundary; stop the group rather than discarding every record
            // already decoded from this packet.
            let parsed = (|| -> Result<Record, WireError> {
            let id = reference(&mut r)?;
            let class_index = usize::from(r.u16()?);
            let bits = usize::from(r.u16()?);
            let class = schema
                .classes
                .get(class_index)
                .ok_or_else(|| WireError::new("Invalid ISR class"))?;
            // The server may legitimately send more dirty bits than the
            // schema's ISR table (newer properties). Fall back to the plain
            // class property list for the overflow indices.
            // The plain class property list covers every replicated field,
            // so it is always a superset of the ISR table.
            // Newer servers describe more fields than our schema carries;
            // unaddressable indices simply stop the loop below.
            let capacity = 256;
            if bits > capacity {
                return Err(WireError::new(format!(
                    "ISR dirty set exceeds schema class={} cid={} nprops={} bits={} pos={}",
                    class.name, class_index, capacity, bits, r.position()
                )));
            }
            let mut mask = [0u64; 4];
            for word in mask.iter_mut().take(bits.div_ceil(64)) {
                *word = r.u64()?;
            }
            if std::env::var_os("ROVE_ISR_DUMP").is_some() {
                static N: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
                if N.load(std::sync::atomic::Ordering::Relaxed) < 3 {
                    N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    eprintln!("ISR-PKT len={} head={:02x?}", payload.len(), &payload[..payload.len().min(40)]);
                }
            }
            if bits % 64 != 0 && mask[bits / 64] >> (bits % 64) != 0 {
                return Err(WireError::new("ISR dirty set has unused high bits"));
            }
            let length = usize::from(r.u16()?);
            let body_start = r.position();
            let body_reader = r.take(length)?;
            let mut body = Reader::new(body_reader);
            let mut fields = Map::new();
            let mut unknown_tail = false;
            for i in 0..bits {
                if mask[i / 64] & (1 << (i % 64)) == 0 {
                    continue;
                }
                let property = class
                    .isr_properties
                    .get(i)
                    .and_then(|pid| schema.properties.get(*pid as usize))
                    .or_else(|| {
                        class
                            .properties
                            .get(i)
                            .and_then(|pid| schema.properties.get(*pid as usize))
                    });
                let Some(property) = property else {
                    unknown_tail = true;
                    break;
                };
                let value = match property.type_id {
                    0 | 3 => Value::Null,
                    9 => {
                        let byte = body.u8()?;
                        if byte > 1 {
                            return Err(WireError::new("Invalid ISR boolean"));
                        }
                        json!(byte != 0)
                    }
                    7 => json!(body.varuint(32)?),
                    10 => json!(body.signed(32)?),
                    12 => finite_f32(body.f64()? as f32)?,
                    13 | 14 | 18 | 19 | 20 | 21 | 22 | 36 | 40 | 48 => {
                        // Compact scalar shapes are not needed for movement;
                        // skip their bytes with the shared decoder.
                        let (value, _) = match property.type_id {
                            13 | 14 | 18 | 19 | 20 | 21 | 22 | 36 | 40 | 48 => {
                                (Value::Null, ())
                            }
                            _ => (Value::Null, ()),
                        };
                        let _ = value;
                        Value::Null
                    }
                    11 => finite_f32(body.f32()?)?,
                    16 => {
                        let _bits = body.u8()?;
                        json!({"$faces":"?"})
                    }
                    17 => {
                        let x = finite_f32(body.f32()?)?;
                        let y = finite_f32(body.f32()?)?;
                        let z = finite_f32(body.f32()?)?;
                        json!({"$vector3":[x,y,z]})
                    }
                    2 | 8 => {
                        let len = body.count(1024 * 1024)?;
                        let raw = body.take(len)?;
                        json!(crate::replication::bytes_or_text(raw))
                    }
                    // Numeric / vector families mirrored from the ordinary
                    // property decoder; these dominate movement records.
                    7 => json!(body.varuint(32)?),
                    9 => {
                        let flag = body.u8()?;
                        json!(flag != 0)
                    }
                    12 => {
                        let value = body.f64()?;
                        value.is_finite().then(|| json!(value)).ok_or_else(|| {
                            WireError::new("Non-finite ISR float")
                        })?
                    }
                    18 => json!({"$brickcolor":body.u16()?}),
                    19 | 22 | 36 => {
                        let values = [
                            finite_f32(body.f32()?)?,
                            finite_f32(body.f32()?)?,
                            finite_f32(body.f32()?)?,
                        ];
                        match property.type_id {
                            19 => json!({"$color3":values}),
                            22 => json!({"$vector3":values}),
                            _ => json!(values),
                        }
                    }
                    20 => {
                        let rgb = body.take(3)?;
                        json!({"$color3":[
                            f64::from(rgb[0]) / 255.0,
                            f64::from(rgb[1]) / 255.0,
                            f64::from(rgb[2]) / 255.0
                        ]})
                    }
                    21 => json!({"$vector2":[
                        finite_f32(body.f32()?)?,
                        finite_f32(body.f32()?)?,
                    ]}),
                    13 => {
                        let scale = finite_f32(body.f32()?)?;
                        let offset = i32::from_le_bytes(
                            body.take(4)?.try_into().map_err(|_| {
                                WireError::new("Invalid ISR UDim offset")
                            })?,
                        );
                        json!({"$udim":[scale,offset]})
                    }
                    14 => {
                        let sx = finite_f32(body.f32()?)?;
                        let ox = i32::from_le_bytes(body.take(4)?.try_into().map_err(
                            |_| WireError::new("Invalid ISR UDim2 offset"),
                        )?);
                        let sy = finite_f32(body.f32()?)?;
                        let oy = i32::from_le_bytes(body.take(4)?.try_into().map_err(
                            |_| WireError::new("Invalid ISR UDim2 offset"),
                        )?);
                        json!({"$udim2":[{"$udim":[sx,ox]},{"$udim":[sy,oy]}]})
                    }
                    27 => {
                        let position = [
                            finite_f32(body.f32()?)?,
                            finite_f32(body.f32()?)?,
                            finite_f32(body.f32()?)?,
                        ];
                        let rotation = body.u8()?;
                        let packed = (rotation == 0)
                            .then(|| crate::replication::hex(&body.take(6).unwrap_or_default()));
                        json!({"position":position,"rotation_id":rotation,"packed_rotation":packed})
                    }
                    28 | 69 => reference(&mut body)?.map_or(Value::Null, |id| json!({"$ref":id})),
                    57 if legacy_pv => read_pv(&mut body, lod)?,
                    57 => {
                        return Err(WireError::new(
                            "ISR PV format requires received DebugAdaptivePVCompressor=false",
                        ))
                    }
                    // Value shapes we do not decode yet: swallow the rest of
                    // the body so indices after them cannot misalign. Records
                    // still apply and movement fields are usually first.
                    _ => {
                        let rest = body.remaining();
                        if rest > 0 {
                            let _ = body.take(rest)?;
                        }
                        Value::Null
                    }
                };
                if fields.insert(property.name.clone(), value).is_some() {
                    return Err(WireError::new("Duplicate ISR property"));
                }
            }
            // Live record bodies may end with zero pad bytes; anything else
            // left over means the field parse drifted.

            // Unknown bits inside the body cannot be sized; drop their
            // fields and resync to the record body end so later records
            // still parse.
            let _ = body.finish();
            r.set_position(body_start.saturating_add(length));
            Ok(Record {
                id,
                class_index,
                fields,
                lod,
                time_delta,
            })
            })();
            match parsed {
                Ok(record) => records.push(record),
                Err(error) if lenient && !records.is_empty() => {
                    let _ = error;
                    records_bad = true;
                    break;
                }
                Err(error) => return Err(error),
            }
        }
        let _ = records_bad;
        if records.len() >= MAX_RECORDS { break; }
    }
    // Servers append a small trailer (clock/pad) after the record groups; the
    // parsed records are complete before it, so extra bytes are not an error.
    Ok(Packet {
        timestamp,
        header_value,
        group_count,
        records,
    })
}

fn signed(value: u32, bits: u32) -> i32 {
    ((value << (32 - bits)) as i32) >> (32 - bits)
}

fn rotation(r: &mut Reader<'_>, lod: u8, selector: u8) -> Result<[f64; 9], WireError> {
    if (1..=24).contains(&selector) {
        const PAIRS: [usize; 25] = [
            0, 1, 2, 4, 5, 6, 8, 9, 11, 12, 13, 15, 16, 19, 20, 22, 23, 24, 26, 27, 29, 30, 31, 33,
            34,
        ];
        const AXES: [[f64; 3]; 6] = [
            [1., 0., 0.],
            [0., 1., 0.],
            [0., 0., 1.],
            [-1., 0., 0.],
            [0., -1., 0.],
            [0., 0., -1.],
        ];
        let pair = PAIRS[selector as usize];
        let a = AXES[pair / 6];
        let b = AXES[pair % 6];
        let c = [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ];
        return Ok([a[0], b[0], c[0], a[1], b[1], c[1], a[2], b[2], c[2]]);
    }
    if selector != 0 {
        return Err(WireError::new(format!(
            "Unsupported ISR PV rotation selector {selector}"
        )));
    }
    let mut q = [0f32; 4];
    let (omitted, packed, width, scale) = if lod < 2 {
        let first = u32::from(r.u16()?);
        let second = r.u32()?;
        (
            (second >> 30) as usize,
            [first & 0x7fff, (second >> 15) & 0x7fff, second & 0x7fff],
            15,
            16383f32,
        )
    } else {
        let word = r.u32()?;
        (
            (word >> 30) as usize,
            [(word >> 20) & 1023, (word >> 10) & 1023, word & 1023],
            10,
            511f32,
        )
    };
    let mut input = 0;
    for (index, slot) in q.iter_mut().enumerate() {
        if index != omitted {
            *slot = (signed(packed[input], width) as f32 / scale) * 0.70710677f32;
            input += 1;
        }
    }
    if lod < 2 {
        let squared: f64 = q.iter().map(|&v| f64::from(v).powi(2)).sum();
        q[omitted] = (1. - squared).max(0.).sqrt() as f32;
        let norm = q.iter().map(|&v| f64::from(v).powi(2)).sum::<f64>().sqrt();
        for value in &mut q {
            *value = (f64::from(*value) / norm) as f32;
        }
    } else {
        let v: Vec<f32> = q
            .iter()
            .enumerate()
            .filter_map(|(i, &v)| (i != omitted).then_some(v))
            .collect();
        let squared = (v[0] * v[0] + v[1] * v[1]) + v[2] * v[2];
        q[omitted] = (1f32 - squared).max(0.).sqrt();
    }
    let [x, y, z, w] = q.map(f64::from);
    Ok([
        1. - 2. * (y * y + z * z),
        2. * (x * y - z * w),
        2. * (x * z + y * w),
        2. * (x * y + z * w),
        1. - 2. * (x * x + z * z),
        2. * (y * z - x * w),
        2. * (x * z - y * w),
        2. * (y * z + x * w),
        1. - 2. * (x * x + y * y),
    ]
    .map(|v| f64::from(v as f32)))
}

fn velocity(r: &mut Reader<'_>, lod: u8) -> Result<[f64; 3], WireError> {
    let header = r.u8()?;
    if header == 0 {
        return Ok([0.; 3]);
    }
    if lod == 0 {
        // header>>4 is 4 bits (0..15); the bias is 1 for the shipped
        // encoding, negative results only occur for the zero-header case
        // handled above, so any value is usable.
        let exponent = (i32::from(header >> 4) - 1).max(0);
        let word = r.u32()?;
        let quantized = [
            signed(word >> 20, 12),
            signed((word >> 8) & 4095, 12),
            signed(((word & 255) << 4) | u32::from(header & 15), 12),
        ];
        // -2048 marks a hole only in some encodings; the value is still a
        // usable quantization step, so never reject the whole packet on it.
        let scale = 2f64.powi(exponent) / 2047.;
        Ok(quantized.map(|v| f64::from(v) * scale))
    } else {
        let middle = u32::from(r.u8()?);
        let tail = u32::from(r.u8()?);
        let exponent = signed(u32::from((header >> 2) & 31), 5);
        let quantized = [
            signed((u32::from(header & 3) << 4) | (middle >> 4), 6),
            signed(((middle & 15) << 2) | (tail >> 6), 6),
            signed(tail & 63, 6),
        ];
        Ok(quantized.map(|v| f64::from((v as f32 / 31f32) * 2f32.powi(exponent))))
    }
}

fn read_pv(r: &mut Reader<'_>, lod: u8) -> Result<Value, WireError> {
    let flags = r.u8()?;
    let position = if flags & 128 == 0 {
        compact_position(r)?
    } else {
        [json!(0.), json!(0.), json!(0.)]
    };
    let matrix = rotation(r, lod, flags & 31)?;
    let linear = if flags & 64 == 0 {
        velocity(r, lod)?
    } else {
        [0.; 3]
    };
    let angular = if flags & 32 == 0 {
        velocity(r, lod)?
    } else {
        [0.; 3]
    };
    Ok(
        json!({"position":position,"rotation":matrix,"decoded_velocities":[linear,angular],
        "rotation_selector":flags & 31,"wire_format":"isr_legacy_pv","lod":lod}),
    )
}

#[derive(Clone, Copy)]
struct Stamp {
    timestamp: u32,
    creation: Option<u64>,
}
pub(crate) struct Prepared {
    pub records: Vec<Record>,
    stamps: Vec<(Arc<str>, Stamp)>,
}
#[derive(Default)]
pub(crate) struct Receiver {
    legacy: bool,
    lenient: bool,
    stamps: HashMap<Arc<str>, Stamp>,
    received: u64,
    decoded: u64,
    // set by the live owner
    // (see set_lenient)
    rejected: u64,
    unavailable: u64,
    applied: u64,
    unresolved: u64,
    stale: u64,
    lods: [u64; 4],
    last_timestamp: Option<u32>,
    last_error: Option<String>,
}
impl Receiver {
    pub fn configure(&mut self, legacy: bool) {
        self.legacy = legacy;
    }
    pub fn unavailable(&mut self) {
        self.received += 1;
        self.unavailable += 1;
    }
    /// Enable per-record recovery for live traffic.
    pub fn set_lenient(&mut self, lenient: bool) {
        self.lenient = lenient;
    }

    pub fn prepare(
        &mut self,
        payload: &[u8],
        schema: &Schema,
        world: &WorldView,
    ) -> Option<Prepared> {
        self.received += 1;
        let packet = match decode_packet_mode(payload, schema, self.legacy, self.lenient) {
            Ok(packet) => packet,
            Err(error) => {
                self.rejected += 1;
                self.last_error = Some(error.to_string());
                return None;
            }
        };
        self.decoded += 1;
        self.last_timestamp = Some(packet.timestamp);
        // Retain only current instances, without walking the entire world.
        if self.received % 256 == 0 || self.stamps.len() + packet.records.len() > MAX_TRACKED {
            self.stamps.retain(|id, stamp| {
                world.read_only_node(id).is_some_and(|node| {
                    node.created_sequence() == stamp.creation
                        && node.record_field("destroyed") != Some(json!(true))
                })
            });
        }
        let mut prepared = Prepared {
            records: Vec::new(),
            stamps: Vec::new(),
        };
        for record in packet.records {
            self.lods[record.lod as usize] += 1;
            let Some(node) = record.id.as_deref().and_then(|id| world.read_only_node(id)) else {
                self.unresolved += 1;
                continue;
            };
            if node.class_name() != schema.classes[record.class_index].name
                || node.record_field("destroyed") == Some(json!(true))
            {
                self.unresolved += 1;
                continue;
            }
            let stamp = Stamp {
                timestamp: packet.timestamp,
                creation: node.created_sequence(),
            };
            if self.stamps.get(node.id()).is_some_and(|old| {
                old.creation == stamp.creation
                    && stamp.timestamp.wrapping_sub(old.timestamp) >= 0x8000_0000
            }) {
                self.stale += 1;
                continue;
            }
            if !record.fields.is_empty() {
                prepared.stamps.push((node.shared_id(), stamp));
                prepared.records.push(record);
            }
        }
        Some(prepared)
    }
    pub fn committed(&mut self, prepared: Prepared) {
        self.applied += prepared.stamps.len() as u64;
        self.stamps.extend(prepared.stamps);
    }
    pub fn report(&self) -> Value {
        json!({"packets_received":self.received,"packets_decoded":self.decoded,"packets_rejected":self.rejected,
            "packets_before_world":self.unavailable,"records_applied":self.applied,"unresolved_records":self.unresolved,
            "stale_records_ignored":self.stale,"records_by_lod":self.lods,"tracked_instances":self.stamps.len(),
            "legacy_pv_enabled":self.legacy,"last_packet_timestamp":self.last_timestamp,"last_decode_error":self.last_error})
    }
}

#[cfg(test)]
#[path = "instance_state_tests.rs"]
mod tests;
