use std::{
    collections::BTreeMap,
    io::{Cursor, Error, ErrorKind},
};

use evalexpr::{context_map, eval_with_context, Value};
use packet_serialize::DeserializePacket;

use crate::{
    game_server::{
        handlers::{
            character::CharacterType,
            item::ItemConfig,
            lock_enforcer::CharacterLockRequest,
            profile::{save_profile, PlayerProfile},
            unique_guid::player_guid,
        },
        packets::{
            client_update::{AddItems, AddItemsData, UpdateCredits},
            item::{Item, MarketData},
            store::{BuyItemRequest, SellToClientResponse, StoreItem, StoreItemList, StoreOpCode},
            tunnel::TunneledPacket,
            GamePacket,
        },
        Broadcast, GameServer, ProcessPacketError, ProcessPacketErrorType,
    },
    ConfigError,
};

/// Handles `OpCode::Store` packets sent by the client.
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

        Ok(StoreOpCode::BuyItem) => {
            let req: BuyItemRequest = DeserializePacket::deserialize(cursor)?;
            // req.unknown is the merchant_id (u64), not a tid to echo back
            process_buy_item(sender, req.unknown, req.item_guid, req.quantity, game_server)
        }

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

fn process_buy_item(
    sender: u32,
    _merchant_id: u64,
    item_guid: u32,
    quantity: u32,
    game_server: &GameServer,
) -> Result<Vec<Broadcast>, ProcessPacketError> {
    let Some(item_config) = game_server.items().get(&item_guid) else {
        return Err(ProcessPacketError::new(
            ProcessPacketErrorType::ConstraintViolated,
            format!("Player {sender} tried to buy unknown item {item_guid}"),
        ));
    };

    let item_def = item_config.to_definition(game_server.abilities());

    game_server
        .lock_enforcer()
        .read_characters(|_| CharacterLockRequest {
            read_guids: vec![],
            write_guids: vec![player_guid(sender)],
            character_consumer: |_, _, mut characters_write, _| {
                let Some(character_write_handle) =
                    characters_write.get_mut(&player_guid(sender))
                else {
                    return Err(ProcessPacketError::new(
                        ProcessPacketErrorType::ConstraintViolated,
                        format!("Unknown player {sender} tried to buy item {item_guid}"),
                    ));
                };

                let CharacterType::Player(ref mut player) =
                    character_write_handle.stats.character_type
                else {
                    return Err(ProcessPacketError::new(
                        ProcessPacketErrorType::ConstraintViolated,
                        format!("Non-player {sender} tried to buy item {item_guid}"),
                    ));
                };

                let cost = if let Some(cost_entry) = game_server.costs().get(&item_guid) {
                    if player.member { cost_entry.members } else { cost_entry.base }
                } else {
                    0
                };

                if cost > player.credits {
                    return Err(ProcessPacketError::new(
                        ProcessPacketErrorType::ConstraintViolated,
                        format!(
                            "Player {sender} tried to buy item {item_guid} for {cost} credits \
                             but only has {}",
                            player.credits
                        ),
                    ));
                }

                player.credits -= cost;
                let new_credits = player.credits;

                player.inventory.add_item(item_guid);
                player.purchased_items.insert(item_guid);

                save_profile(
                    game_server.profiles_dir(),
                    sender,
                    &PlayerProfile::from_player(
                        &player.name,
                        &player.inventory,
                        &player.customizations,
                        &player.collected_items,
                        &player.fav_emotes,
                        &player.purchased_items,
                        player.credits,
                    ),
                );

                let item = Item {
                    definition_id: item_guid,
                    tint: item_config.tint,
                    guid: item_guid,
                    quantity: quantity.max(1),
                    num_consumed: 0,
                    last_use_time: 0,
                    market_data: MarketData::None,
                    unknown2: false,
                };

                Ok(vec![Broadcast::Single(
                    sender,
                    vec![
                        GamePacket::serialize(&TunneledPacket {
                            unknown1: true,
                            inner: AddItems {
                                data: AddItemsData {
                                    item,
                                    definition: item_def,
                                },
                            },
                        }),
                        GamePacket::serialize(&TunneledPacket {
                            unknown1: true,
                            inner: UpdateCredits { new_credits },
                        }),
                        // Sub-opcode 6: CoinStoreSellToClientResponsePacket.
                        // Calls Lua MerchantPurchased / MerchantPurchasedOne,
                        // which close CWAStoreWindowSelectedItem.
                        // tid is u32 (confirmed: %d format in binary, not %I64u).
                        // BuyItemRequest.unknown is the merchant ID (u64), not a
                        // client-generated tid to echo back; we use 0.
                        GamePacket::serialize(&TunneledPacket {
                            unknown1: true,
                            inner: SellToClientResponse {
                                result: 0,
                                tid: 0,
                                transaction_type: 0,
                                item_guids: vec![item_guid],
                                quantity: quantity.max(1),
                            },
                        }),
                        // Refresh store item list.
                        GamePacket::serialize(&TunneledPacket {
                            unknown1: true,
                            inner: StoreItemList::from(game_server.costs()),
                        }),
                    ],
                )])
            },
        })
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
