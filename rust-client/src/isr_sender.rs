//! Outgoing instance-state records using the session's received ISR schema.
//! Unknown remote/local clock offset is represented explicitly, never guessed.
use crate::{owned_part_poses::PartFrame, physics_codec::{encode_position, encode_velocity}};
use rove_client_core::{schema::Schema, world::WorldView};

pub(crate) const APPLICATION: u8 = 4;
pub(crate) const CHANNEL: u8 = 5;
pub(crate) const WIRE_ID: u32 = 10;
const MAX_DATAGRAM: usize = 1100;

pub(crate) fn record(schema: &Schema, view: &WorldView, frame: &PartFrame) -> Result<Vec<u8>, &'static str> {
    let id = format!("{}:{}", frame.id >> 32, frame.id as u32);
    let name = view.class_name(&id).ok_or("isr-part-unavailable")?;
    if std::env::var_os("ROVE_ISR_SEND_TRACE").is_some() {
        eprintln!("ISR-SEND part={} name={} scope_owner={:?}", id, name,
            view.get_property(&id, "NetworkOwnerV3"));
    }
    let class_index = schema.classes.iter().position(|class| class.name == name).ok_or("isr-class-unavailable")?;
    let class = &schema.classes[class_index];
    if class.id != class_index || class_index > u16::MAX as usize || class.isr_properties.is_empty()
        || class.isr_properties.len() > 256 { return Err("isr-property-table-invalid"); }
    let mut selected = None;
    for (bit, &index) in class.isr_properties.iter().enumerate() {
        let property = schema.properties.get(index as usize).ok_or("isr-property-index-invalid")?;
        if property.name == "ReplicationPV" {
            if property.type_id != 57 || selected.replace(bit).is_some() { return Err("isr-pv-schema-ambiguous"); }
        }
    }
    let bit = selected.ok_or("isr-pv-schema-unavailable")?;
    let mut body = vec![0]; // LOD 0, explicit position/rotation/both velocities
    body.extend(encode_position(frame.position).map_err(|_| "isr-position-invalid")?);
    body.extend(crate::physics_sender::encode_rotation(frame.rotation)?);
    body.extend(encode_velocity(frame.linear).map_err(|_| "isr-velocity-invalid")?);
    body.extend(encode_velocity(frame.angular).map_err(|_| "isr-velocity-invalid")?);
    let mut output = Vec::with_capacity(80);
    crate::physics_sender::reference(&mut output, (frame.id >> 32) as u32, frame.id as u32)?;
    output.extend((class_index as u16).to_le_bytes());
    output.extend((class.isr_properties.len() as u16).to_le_bytes());
    for word in 0..class.isr_properties.len().div_ceil(64) {
        output.extend((if word == bit / 64 { 1u64 << (bit % 64) } else { 0 }).to_le_bytes());
    }
    output.extend((body.len() as u16).to_le_bytes());
    output.extend(body);
    Ok(output)
}

pub(crate) fn packet(local_step: u32, records: &[Vec<u8>]) -> Result<Vec<u8>, &'static str> {
    packet_with_offset(local_step,records,None)
}
pub(crate) fn packet_with_offset(local_step:u32,records:&[Vec<u8>],offset:Option<i32>)->Result<Vec<u8>, &'static str> {
    if offset.is_some_and(|v| !(-0x7fff_fffe..=0x7fff_fffe).contains(&v)) { return Err("isr-clock-reserved-offset"); }
    if records.is_empty() || records.len() > 8 || records.iter().any(Vec::is_empty)
        || 17 + records.iter().map(Vec::len).sum::<usize>() > MAX_DATAGRAM {
        return Err("isr-packet-batch-invalid");
    }
    let mut output = Vec::with_capacity(17 + records.iter().map(Vec::len).sum::<usize>());
    output.push(161);
    output.extend(local_step.to_le_bytes());
    output.extend(1u32.to_le_bytes()); // one group
    output.extend(offset.map_or(i32::MAX, |v| -v).to_le_bytes()); // unknown until actual delivery history
    output.extend((records.len() as u16).to_le_bytes());
    output.extend([0, 0]); // LOD 0, current state at this group's own local step
    for record in records { output.extend(record); }
    Ok(output)
}

#[cfg(test)]
#[path = "isr_sender_tests.rs"]
mod tests;
