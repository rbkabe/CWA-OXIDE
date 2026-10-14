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
            companion::{build_pet_inventory_packets, is_companion_item},
            item::ItemConfig,
            lock_enforcer::CharacterLockRequest,
            profile::{save_profile, PlayerProfile},
            unique_guid::player_guid,
        },
        packets::{
            client_update::{AddItems, AddItemsData, UpdateCredits},
            item::{Item, ItemDefinition, MarketData},
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

        // Client sends a count-prefixed list of item GUIDs when the store
        // window opens, requesting definitions (sub-opcode 2 → reply 3).
        // The correct format of the sub-opcode 3 reply is not yet confirmed,
        // so we consume the packet silently rather than send a wrong response
        // that could corrupt the client's store state.
        Ok(StoreOpCode::ItemDefinitionsRequest) => {
            let count: u32 = DeserializePacket::deserialize(cursor)?;
            for _ in 0..count {
                let _guid: u32 = DeserializePacket::deserialize(cursor)?;
            }
            Ok(vec![])
        }

        Ok(StoreOpCode::BuyItem) => {
            let req: BuyItemRequest = DeserializePacket::deserialize(cursor)?;
            crate::info!(
                "BuyItem from {sender}: merchant_id={} item_guid={} currency={} quantity={}",
                req.unknown, req.item_guid, req.currency_type, req.quantity
            );
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
        // Companion items live in purchased_items/companions but have no entry
        // in the regular items config.  Handle them before falling through to
        // process_buy_group — which would find the *accessories pack* group
        // that shares the same GUID (e.g. group 633 = T0-D0 accessory set),
        // not the companion itself.
        if is_companion_item(item_guid) {
            return process_buy_companion(sender, item_guid, quantity, game_server);
        }
        // Not a single item — fall back to item-group purchase.
        return process_buy_group(sender, item_guid, quantity, game_server);
    };

    let item_def = item_config.to_definition(game_server.abilities());
    let item_tint = item_config.tint;

    let broadcasts = game_server
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

                // Snapshot pet state for the PetInventory refresh below.
                let active_pet_id = character_write_handle.stats.active_companion_item_guid.unwrap_or(0);
                let active_companion_guid = character_write_handle.stats.companion_guid.unwrap_or(0);
                let companion_attachments = character_write_handle.stats.companion_attachments.clone();

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
                let offhand_packet = grant_offhand_hilt(&mut player.inventory, item_guid, game_server);

                // Companions need to be in player.companions so owns_companion passes.
                if is_companion_item(item_guid) && !player.companions.contains(&item_guid) {
                    player.companions.push(item_guid);
                }

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
                    tint: item_tint,
                    guid: item_guid,
                    quantity: quantity.max(1),
                    num_consumed: 0,
                    last_use_time: 0,
                    market_data: MarketData::None,
                    unknown2: false,
                };

                let packets = vec![
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
                    GamePacket::serialize(&TunneledPacket {
                        unknown1: true,
                        inner: SellToClientResponse {
                            result: 1, // 1 = success (client logs anything else as a failed transaction)
                            tid: next_transaction_id(),
                            transaction_type: 1, // 1 = buy → HandlerCoinStore.OnBuy (0 = no callback)
                            item_guids: vec![item_guid],
                            quantity: quantity.max(1),
                        },
                    }),
                    // Refresh store item list.
                    GamePacket::serialize(&TunneledPacket {
                        unknown1: true,
                        inner: StoreItemList::from(game_server.costs()),
                    }),
                ];
                let mut packets = packets;
                if let Some(offhand_packet) = offhand_packet {
                    packets.insert(1, offhand_packet);
                }
                if is_companion_item(item_guid) {
                    packets.extend(pet_list_refresh(active_pet_id, active_companion_guid, &player.purchased_items, &companion_attachments));
                }
                if super::mount::is_mount_item(item_guid, game_server) {
                    packets.push(super::mount::mount_list_packet(sender, &player.purchased_items, game_server));
                }

                Ok(vec![Broadcast::Single(sender, packets)])
            },
        })?;

    // The companion list was refreshed with PetInventory inside the lock.
    // Resending the full Player packet mid-session crashes the client.
    Ok(broadcasts)
}

/// Handles a BuyItem request for a companion item that has no entry in the
/// regular items config.  Adds the companion GUID directly to the player's
/// `purchased_items` and `companions`, saves the profile, and sends
/// `SellToClientResponse` + `StoreItemList`.  No `AddItems` packet is sent
/// because we have no `ItemDefinition` for companions; the companion will
/// appear in the flyout after the next login.
fn process_buy_companion(
    sender: u32,
    item_guid: u32,
    quantity: u32,
    game_server: &GameServer,
) -> Result<Vec<Broadcast>, ProcessPacketError> {
    let broadcasts = game_server
        .lock_enforcer()
        .read_characters(|_| CharacterLockRequest {
            read_guids: vec![],
            write_guids: vec![player_guid(sender)],
            character_consumer: |_, _, mut characters_write, _| {
                let Some(char_write) = characters_write.get_mut(&player_guid(sender)) else {
                    return Err(ProcessPacketError::new(
                        ProcessPacketErrorType::ConstraintViolated,
                        format!("Unknown player {sender} tried to buy companion {item_guid}"),
                    ));
                };

                // Snapshot pet state for the PetInventory refresh below.
                let active_pet_id = char_write.stats.active_companion_item_guid.unwrap_or(0);
                let active_companion_guid = char_write.stats.companion_guid.unwrap_or(0);
                let companion_attachments = char_write.stats.companion_attachments.clone();

                let CharacterType::Player(ref mut player) = char_write.stats.character_type else {
                    return Err(ProcessPacketError::new(
                        ProcessPacketErrorType::ConstraintViolated,
                        format!("Non-player {sender} tried to buy companion {item_guid}"),
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
                            "Player {sender} tried to buy companion {item_guid} for {cost} \
                             credits but only has {}",
                            player.credits
                        ),
                    ));
                }

                player.credits -= cost;
                let new_credits = player.credits;

                player.inventory.add_item(item_guid);
                player.purchased_items.insert(item_guid);
                if !player.companions.contains(&item_guid) {
                    player.companions.push(item_guid);
                }

                crate::info!(
                    "Companion {item_guid} purchased by player {sender}: \
                     added to purchased_items + companions, credits now {new_credits}"
                );

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

                let packets = vec![
                    GamePacket::serialize(&TunneledPacket {
                        unknown1: true,
                        inner: UpdateCredits { new_credits },
                    }),
                    GamePacket::serialize(&TunneledPacket {
                        unknown1: true,
                        inner: SellToClientResponse {
                            result: 1, // 1 = success (client logs anything else as a failed transaction)
                            tid: next_transaction_id(),
                            transaction_type: 1, // 1 = buy → HandlerCoinStore.OnBuy (0 = no callback)
                            item_guids: vec![item_guid],
                            quantity: quantity.max(1),
                        },
                    }),
                    GamePacket::serialize(&TunneledPacket {
                        unknown1: true,
                        inner: StoreItemList::from(game_server.costs()),
                    }),
                ];
                let mut packets = packets;
                packets.extend(pet_list_refresh(active_pet_id, active_companion_guid, &player.purchased_items, &companion_attachments));

                Ok(vec![Broadcast::Single(sender, packets)])
            },
        })?;

    Ok(broadcasts)
}

/// Handles a BuyItem request where `group_guid` is an item-group GUID rather than
/// a single item GUID.  Buys every item in the group at once, deducting the total
/// cost (pre-computed in `load_item_groups` and stored in the costs map).
pub fn process_buy_group(
    sender: u32,
    group_guid: u32,
    quantity: u32,
    game_server: &GameServer,
) -> Result<Vec<Broadcast>, ProcessPacketError> {
    // Find the matching group definition.
    let group = game_server
        .item_groups()
        .definitions
        .iter()
        .find(|g| g.guid as u32 == group_guid);

    let Some(group) = group else {
        return Err(ProcessPacketError::new(
            ProcessPacketErrorType::ConstraintViolated,
            format!("Player {sender} tried to buy unknown item or group {group_guid}"),
        ));
    };

    // Pre-collect item data (Item + ItemDefinition) so they can be moved into the closure.
    let group_items: Vec<(u32, Item, ItemDefinition)> = group
        .items
        .iter()
        .filter_map(|gi| {
            let cfg = game_server.items().get(&gi.guid)?;
            let definition = cfg.to_definition(game_server.abilities());
            let item = Item {
                definition_id: gi.guid,
                tint: cfg.tint,
                guid: gi.guid,
                quantity: quantity.max(1),
                num_consumed: 0,
                last_use_time: 0,
                market_data: MarketData::None,
                unknown2: false,
            };
            Some((gi.guid, item, definition))
        })
        .collect();

    // Separate list of guids for SellToClientResponse (group_items is moved later).
    let item_guids: Vec<u32> = group_items.iter().map(|(guid, _, _)| *guid).collect();

    game_server
        .lock_enforcer()
        .read_characters(|_| CharacterLockRequest {
            read_guids: vec![],
            write_guids: vec![player_guid(sender)],
            character_consumer: |_, _, mut characters_write, _| {
                let Some(char_write) = characters_write.get_mut(&player_guid(sender)) else {
                    return Err(ProcessPacketError::new(
                        ProcessPacketErrorType::ConstraintViolated,
                        format!("Unknown player {sender} tried to buy group {group_guid}"),
                    ));
                };

                // Snapshot pet state for the PetInventory refresh below.
                let active_pet_id = char_write.stats.active_companion_item_guid.unwrap_or(0);
                let active_companion_guid = char_write.stats.companion_guid.unwrap_or(0);
                let companion_attachments = char_write.stats.companion_attachments.clone();

                let CharacterType::Player(ref mut player) = char_write.stats.character_type else {
                    return Err(ProcessPacketError::new(
                        ProcessPacketErrorType::ConstraintViolated,
                        format!("Non-player {sender} tried to buy group {group_guid}"),
                    ));
                };

                let cost = if let Some(entry) = game_server.costs().get(&group_guid) {
                    if player.member { entry.members } else { entry.base }
                } else {
                    0
                };

                if cost > player.credits {
                    return Err(ProcessPacketError::new(
                        ProcessPacketErrorType::ConstraintViolated,
                        format!(
                            "Player {sender} tried to buy group {group_guid} for {cost} credits \
                             but only has {}",
                            player.credits
                        ),
                    ));
                }

                player.credits -= cost;
                let new_credits = player.credits;

                let mut offhand_packets = Vec::new();
                for (item_guid, _, _) in &group_items {
                    player.inventory.add_item(*item_guid);
                    player.purchased_items.insert(*item_guid);
                    offhand_packets.extend(grant_offhand_hilt(&mut player.inventory, *item_guid, game_server));
                    if is_companion_item(*item_guid) && !player.companions.contains(item_guid) {
                        player.companions.push(*item_guid);
                    }
                }

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

                let mut packets: Vec<Vec<u8>> = group_items
                    .into_iter()
                    .map(|(_, item, definition)| {
                        GamePacket::serialize(&TunneledPacket {
                            unknown1: true,
                            inner: AddItems {
                                data: AddItemsData { item, definition },
                            },
                        })
                    })
                    .collect();
                packets.append(&mut offhand_packets);

                packets.push(GamePacket::serialize(&TunneledPacket {
                    unknown1: true,
                    inner: UpdateCredits { new_credits },
                }));
                for response in SellToClientResponse::per_item(1, 1, &item_guids, quantity.max(1), next_transaction_id) {
                    packets.push(GamePacket::serialize(&TunneledPacket {
                        unknown1: true,
                        inner: response,
                    }));
                }
                packets.push(GamePacket::serialize(&TunneledPacket {
                    unknown1: true,
                    inner: StoreItemList::from(game_server.costs()),
                }));
                if item_guids.iter().any(|g| is_companion_item(*g)) {
                    packets.extend(pet_list_refresh(active_pet_id, active_companion_guid, &player.purchased_items, &companion_attachments));
                }
                if item_guids.iter().any(|g| super::mount::is_mount_item(*g, game_server)) {
                    packets.push(super::mount::mount_list_packet(sender, &player.purchased_items, game_server));
                }

                Ok(vec![Broadcast::Single(sender, packets)])
            },
        })
}

/// AddItems packet granting the offhand copy of a purchased one-handed hilt
/// (owning the primary hilt grants it; see `GameServer::offhand_hilt`).
fn grant_offhand_hilt(
    player_inventory: &mut super::character::PlayerInventory,
    primary_hilt: u32,
    game_server: &GameServer,
) -> Option<Vec<u8>> {
    let offhand = game_server.offhand_hilt(primary_hilt)?;
    let config = game_server.items().get(&offhand)?;
    player_inventory.add_item(offhand);
    Some(GamePacket::serialize(&TunneledPacket {
        unknown1: true,
        inner: AddItems {
            data: AddItemsData {
                item: Item {
                    definition_id: offhand,
                    tint: config.tint,
                    guid: offhand,
                    quantity: 1,
                    num_consumed: 0,
                    last_use_time: 0,
                    market_data: MarketData::None,
                    unknown2: false,
                },
                definition: config.to_definition(game_server.abilities()),
            },
        },
    }))
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

/// Sends PetInventory so a newly bought companion appears in BaseClient.PetList,
/// keeping the currently active pet (if any) active.  The pet HUD windows are
/// deliberately NOT re-shown here: the store hides the HUD while open, and the
/// client's own PetController:show restores the active pet window when the
/// store closes (PetsHandler.activePetId is set by the client from PetInventory).
fn pet_list_refresh(
    active_pet_id: u32,
    companion_guid: u64,
    owned: &std::collections::HashSet<u32>,
    equipped: &std::collections::HashMap<u32, u32>,
) -> Vec<Vec<u8>> {
    build_pet_inventory_packets(active_pet_id, companion_guid, owned, equipped)
}

/// Non-zero transaction ids for SellToClientResponse: the client only fires the
/// MerchantPurchased / MerchantPurchasedOne Lua events when tid != 0.
fn next_transaction_id() -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}
