use std::{
    collections::BTreeMap,
    io::{Cursor, Error, ErrorKind},
};

use evalexpr::{context_map, eval_with_context, Value};
use packet_serialize::DeserializePacket;

use crate::{
    game_server::{
        handlers::item::ItemConfig,
        packets::{
            store::{StoreItem, StoreItemList, StoreOpCode},
            tunnel::TunneledPacket,
            GamePacket,
        },
        Broadcast, GameServer, ProcessPacketError, ProcessPacketErrorType,
    },
    ConfigError,
};

/// Handles `OpCode::Store` packets sent by the client.
///
/// Currently only `RequestItemList` (sub-opcode 8) is confirmed to exist on
/// the wire (captured live when opening the Gear/Store tab, with an empty
/// body). EMPIRICAL/UNVERIFIED: we respond with the same `StoreItemList`
/// that's already pushed unconditionally at login, on the guess that this
/// is a "refresh the store" request. If the client doesn't visibly update,
/// the next things to try are `ItemDefinitionsReply` (sub-opcode 3) or one
/// of the other sub-opcodes the client can receive (6, 7, 9, 10, 11, 13, 17,
/// 18 -- see `StoreOpCode` doc comment).
pub fn process_store_packet(
    cursor: &mut Cursor<&[u8]>,
    sender: u32,
    game_server: &GameServer,
) -> Result<Vec<Broadcast>, ProcessPacketError> {
    let raw_op_code: u16 = DeserializePacket::deserialize(cursor)?;
    match StoreOpCode::try_from(raw_op_code) {
        Ok(StoreOpCode::RequestItemList) => Ok(vec![Broadcast::Single(
            sender,
            vec![GamePacket::serialize(&TunneledPacket {
                unknown1: true,
                inner: StoreItemList::from(game_server.costs()),
            })],
        )]),
        Ok(op_code) => {
            let remaining = &cursor.get_ref()[cursor.position() as usize..];
            Err(ProcessPacketError::new(
                ProcessPacketErrorType::UnknownOpCode,
                format!(
                    "Unimplemented store op code: {op_code:?}, remaining bytes: {remaining:x?}"
                ),
            ))
        }
        Err(_) => {
            let remaining = &cursor.get_ref()[cursor.position() as usize..];
            Err(ProcessPacketError::new(
                ProcessPacketErrorType::UnknownOpCode,
                format!("Unknown store op code: {raw_op_code}, remaining bytes: {remaining:x?}"),
            ))
        }
    }
}

pub struct CostEntry {
    pub base: u32,
    pub members: u32,
}

pub type ItemCostMap = BTreeMap<u32, CostEntry>;

pub fn compute_costs(items: &[ItemConfig]) -> Result<BTreeMap<u32, CostEntry>, ConfigError> {
    let mut costs = BTreeMap::new();

    for item_config in items.iter() {
        let cost_entry = costs.entry(item_config.guid).or_insert_with(|| CostEntry {
            base: item_config.cost,
            members: item_config.cost,
        });

        cost_entry.base = item_config.cost;
        cost_entry.members = evaluate_cost_expression(
            &item_config.members_cost_expression,
            cost_entry.members,
            item_config.guid,
        )?;
    }

    Ok(costs)
}

impl From<&BTreeMap<u32, CostEntry>> for StoreItemList {
    fn from(cost_map: &BTreeMap<u32, CostEntry>) -> Self {
        StoreItemList {
            static_items: cost_map
                .iter()
                .map(|(item_guid, costs)| StoreItem {
                    guid: *item_guid,
                    unknown2: 0,
                    unknown3: 0,
                    unknown4: false,
                    unknown5: false,
                    unknown6: 0,
                    unknown7: false,
                    unknown8: false,
                    base_cost: costs.base,
                    unknown10: 0,
                    unknown11: 0,
                    unknown12: 0,
                    member_cost: costs.members,
                })
                .collect(),
            dynamic_items: vec![],
        }
    }
}

fn evaluate_cost_expression(
    cost_expression: &str,
    cost: u32,
    item_guid: u32,
) -> Result<u32, Error> {
    let context = context_map! {
        "x" => evalexpr::Value::Float(cost as f64),
    }
    .unwrap_or_else(|_| {
        panic!("Couldn't build expression evaluation context for item {item_guid}")
    });

    let result = eval_with_context(cost_expression, &context).map_err(|err| {
        Error::new(
            ErrorKind::InvalidData,
            format!("Unable to evaluate cost expression for item {item_guid}: {err}"),
        )
    })?;

    let Value::Float(new_cost) = result else {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!(
                "Cost expression did not return an integer for item {item_guid}, returned: {result}"
            ),
        ));
    };

    u32::try_from(new_cost.round() as i64).map_err(|err| {
        Error::new(
            ErrorKind::InvalidData,
            format!(
                "Cost expression returned float that could not be converted to an integer for item {item_guid}: {new_cost}, {err}"
            ),
        )
    })
}
