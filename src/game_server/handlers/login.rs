use std::collections::BTreeMap;

use num_enum::TryFromPrimitive;
use packet_serialize::NullTerminatedString;

use crate::{
    game_server::{
        handlers::{
            character::{
                BattleClass, PlayerAbilityGroup, PlayerActionBar, PlayerInventory, Toggles,
            },
            minigame::{leave_active_minigame_if_any, LeaveMinigameTarget},
        },
        packets::{
            item::ItemDefinition,
            login::{DefinePointsOfInterest, DeploymentEnv, GameSettings, LoginReply},
            player_update::ItemDefinitionsReply,
            tunnel::TunneledPacket,
            GamePacket,
        },
        Broadcast, GameServer, ProcessPacketError, ProcessPacketErrorType,
    },
    info,
};

use super::{
    character::{Character, Player, PreviousLocation, RemovalMode, Role},
    guid::IndexedGuid,
    lock_enforcer::ZoneLockEnforcer,
    minigame::PlayerMinigameStats,
    profile::load_profile,
    test_data::{make_test_collection_replay, make_test_customizations, make_test_player},
    unique_guid::player_guid,
    zone::{clean_up_zone_if_no_players, ZoneInstance},
};

/// Builds the player's starting weapon ability groups from whatever is
/// actually equipped in their active battle class, mirroring the same
/// priority logic the live equip flow uses in inventory.rs (item_def's
/// action_bar.priority_override, falling back to the EquipmentSlot's
/// action_bar_priority()). This replaces the old approach of unconditionally
/// injecting a hardcoded test-only ability group, which had no real item
/// backing it and could never be displaced by real equips.
fn derive_initial_weapon_abilities(
    inventory: &PlayerInventory,
    active_battle_class: u32,
    game_server: &GameServer,
) -> Vec<PlayerAbilityGroup> {
    inventory
        .equipped_items(active_battle_class)
        .into_iter()
        .filter_map(|(slot, item_guid)| {
            let item_def = game_server.items().get(&item_guid)?;
            if item_def.action_bar.ability_keys.is_empty() {
                return None;
            }

            Some(PlayerAbilityGroup {
                source_item_id: item_def.guid,
                ability_keys: item_def.action_bar.ability_keys.clone(),
                priority: item_def
                    .action_bar
                    .priority_override
                    .unwrap_or_else(|| slot.action_bar_priority()),
            })
        })
        .collect()
}

pub fn log_in(sender: u32, game_server: &GameServer) -> Result<Vec<Broadcast>, ProcessPacketError> {
    crate::info!("Player {} logging in", sender);
    game_server.lock_enforcer().write_characters(
        |characters_table_write_handle, minigame_data_lock_enforcer| {
            let zones_lock_enforcer: ZoneLockEnforcer<'_> = minigame_data_lock_enforcer.into();
            // TODO: get player's zone
            let player_zone_template = 24;

            let mut packets = Vec::new();

            let login_reply = TunneledPacket {
                unknown1: true,
                inner: LoginReply { logged_in: true },
            };
            packets.push(GamePacket::serialize(&login_reply));

            let deployment_env = TunneledPacket {
                unknown1: true,
                inner: DeploymentEnv {
                    environment: NullTerminatedString("prod".to_string()),
                },
            };
            packets.push(GamePacket::serialize(&deployment_env));

            let (instance_guid, mut zone_packets) =
                zones_lock_enforcer.write_zones(|zones_table_write_handle| {
                    let instance_guid = game_server.get_or_create_instance(
                        characters_table_write_handle,
                        zones_table_write_handle,
                        player_zone_template,
                        1,
                    )?;
                    let zone_read_handle =
                        zones_table_write_handle.get(instance_guid).unwrap().read();
                    Ok::<(u64, Vec<Vec<u8>>), ProcessPacketError>((
                        zone_read_handle.guid(),
                        zone_read_handle.send_self(sender)?,
                    ))
                })?;
            packets.append(&mut zone_packets);

            let settings = TunneledPacket {
                unknown1: true,
                inner: GameSettings {
                    unknown1: 4,
                    unknown2: 7,
                    unknown3: 268,
                    unknown4: true,
                    time_scale: 1.0,
                },
            };
            packets.push(GamePacket::serialize(&settings));

            let item_defs: BTreeMap<u32, ItemDefinition> = game_server
                .items()
                .iter()
                .map(|(id, config)| (*id, config.to_definition(game_server.abilities())))
                .collect();

            let item_defs_reply = TunneledPacket {
                unknown1: true,
                inner: ItemDefinitionsReply {
                    definitions: &item_defs,
                },
            };
            packets.push(GamePacket::serialize(&item_defs_reply));

            // Load per-player profile and build the player packet, applying any
            // saved overrides on top of the test-data defaults.
            let profile = load_profile(game_server.profiles_dir(), sender);

            let mut player_data = make_test_player(sender, game_server.mounts(), game_server.items());

            // Override equipped items from saved profile.
            for (bc_guid, slot_map) in &profile.equipped {
                if let Some(bc) = player_data.data.battle_classes.get_mut(bc_guid) {
                    for (slot_u32, item_guid) in slot_map {
                        if let Ok(slot) =
                            crate::game_server::packets::item::EquipmentSlot::try_from_primitive(*slot_u32)
                        {
                            bc.items.insert(
                                slot,
                                crate::game_server::packets::player_data::EquippedItem {
                                    slot,
                                    guid: *item_guid,
                                    category: 0,
                                },
                            );
                        }
                    }
                }
            }

            // Override name from saved profile.
            if let Some(ref first) = profile.first_name {
                player_data.data.name.first_name = first.clone();
            }
            if let Some(ref last) = profile.last_name {
                player_data.data.name.last_name = last.clone();
            }

            // Override credits from saved profile. New/anonymous players
            // (no profile file) keep the test-data default. Profiled players
            // use their persisted balance, defaulting to 100,000 on first login.
            const DEFAULT_STARTING_CREDITS: u32 = 100_000;
            if profile.first_name.is_some() {
                player_data.data.credits =
                    profile.credits.unwrap_or(DEFAULT_STARTING_CREDITS);
            }

            // Restrict client-visible inventory for profiled players: only send
            // items they actually own, not the full test-data set.  New players
            // (first_name is None) keep the full inventory so hosts can test
            // everything.
            if profile.first_name.is_some() {
                player_data.data.inventory.retain(|guid, _| {
                    profile.purchased_items.contains(guid)
                });
            }

            let player = TunneledPacket {
                unknown1: true,
                inner: player_data,
            };
            packets.push(GamePacket::serialize(&player));

            let Some(zone_template) = game_server.read_zone_templates().get(&player_zone_template)
            else {
                return Err(ProcessPacketError::new(
                    ProcessPacketErrorType::ConstraintViolated,
                    format!(
                        "Player {sender} tried to log in at zone template {player_zone_template} that doesn't exist"
                    ),
                ));
            };

            // If a real on-disk profile exists (first_name is Some), the player's
            // inventory is exactly what they own: only purchased items.  New players
            // (no profile file yet) fall back to the full test-data set so the
            // host can still access everything.
            let mut base_inventory: std::collections::BTreeSet<u32> =
                if profile.first_name.is_some() {
                    profile.purchased_items.iter().copied().collect()
                } else {
                    player.inner.data.inventory.into_keys().collect()
                };
            // Always ensure purchased items are present even for the full-inventory path.
            for item_guid in &profile.purchased_items {
                base_inventory.insert(*item_guid);
            }
            let inventory = PlayerInventory::new(
                player
                    .inner
                    .data
                    .battle_classes
                    .into_iter()
                    .map(|(battle_class_guid, battle_class)| {
                        (
                            battle_class_guid,
                            BattleClass {
                                items: battle_class.items.into_iter()
                                    .map(|(slot, item)| (slot, item.guid))
                                    .collect(),
                            },
                        )
                    })
                    .collect(),
                player.inner.data.active_battle_class,
                base_inventory,
            );

            let weapon_abilities = derive_initial_weapon_abilities(
                &inventory,
                player.inner.data.active_battle_class,
                game_server,
            );

            // NOTE: Don't send EquipItem/UnequipItem packets here. The client
            // isn't ready to receive them yet at this point in the login
            // sequence (it crashes -- confirmed by testing). The existing
            // OpCode::ClientIsReady handler in mod.rs is where equip state is
            // sent to the client once it has actually finished loading (see
            // the update_saber_tints call there) -- that's also where the
            // full-inventory EquipItem broadcast for the Gear tab belongs.

            characters_table_write_handle.insert(Character::from_player(
                sender,
                player.inner.data.body_model,
                player.inner.data.pos,
                player.inner.data.rot,
                zone_template.chunk_size,
                instance_guid,
                Player {
                    first_load: true,
                    ready: false,
                    name: player.inner.data.name,
                    squad_guid: None,
                    member: player.inner.data.membership_unknown1,
                    credits: player.inner.data.credits,
                    inventory,
                    customizations: if profile.customizations.is_empty() {
                        make_test_customizations()
                    } else {
                        profile.customizations.iter()
                            .filter_map(|(slot_i32, guid)| {
                                crate::game_server::packets::player_update::CustomizationSlot
                                    ::try_from_primitive(*slot_i32)
                                    .ok()
                                    .map(|slot| (slot, *guid))
                            })
                            .collect()
                    },
                    minigame_stats: PlayerMinigameStats::default(),
                    minigame_status: None,
                    update_previous_location_on_leave: true,
                    previous_location: PreviousLocation {
                        template_guid: player_zone_template,
                        pos: player.inner.data.pos,
                        rot: player.inner.data.rot,
                    },
                    toggles: Toggles {
                        console: false,
                        free_camera: false,
                        click_to_teleport: false,
                    },
                    role: Role::Admin,
                    collected_items: profile.collected_items,
                    started_collections: std::collections::HashSet::new(),
                    // Pre-populate with hardcoded test data so the Collections
                    // window is testable at login without NPC interaction.
                    // Remove once the real flow is confirmed working.
                    collection_replay: make_test_collection_replay(),
                    fav_emotes: profile.fav_emotes.into_iter().collect(),
                    companions: player.inner.data.pets.iter()
                        .filter_map(|pet| pet.item_guid.first().map(|ig| ig.guid))
                        .collect(),
                    purchased_items: profile.purchased_items,
                    action_bar: PlayerActionBar {
                        // Root cause of "new weapons land on slot 4 instead of
                        // replacing slot 1": this used to unconditionally call
                        // make_test_weapon_abilities(), which hardcodes a fake
                        // ability group (source_item_id 2909: vigilance,
                        // thermal_grenade, focused_shot) at priority 2 for
                        // EVERY login, regardless of what's actually equipped.
                        // The real equip flow (inventory.rs) only adds/removes
                        // groups by matching source_item_id against the items
                        // actually being equipped/unequipped, so it never
                        // touches this phantom entry - it has no real item
                        // backing it. Since real weapons (e.g. a lightsaber in
                        // PrimaryWeapon, priority 2) get pushed AFTER it with
                        // the same priority, the stub's 3 keys permanently
                        // occupy slots 0-2 and every real weapon's abilities
                        // get appended starting at slot 3/4 instead of
                        // replacing slot 0.
                        //
                        // Fix: derive the initial ability groups from the
                        // player's actual equipped items, the same way
                        // inventory.rs does on a live equip.
                        weapon_abilities,
                        consumable_slots: [None; 4],
                    },
                },
                game_server,
            ));

            Ok(vec![Broadcast::Single(sender, packets)])
        },
    )
}

pub fn log_out(sender: u32, game_server: &GameServer) -> Vec<Broadcast> {
    info!("Logging out player {}", sender);
    game_server.lock_enforcer().write_characters(
        |characters_table_write_handle, minigame_data_lock_enforcer| {
            minigame_data_lock_enforcer.write_minigame_data(
                |minigame_data_write_handle, zones_lock_enforcer| {
                    zones_lock_enforcer.write_zones(|zones_table_write_handle| {
                        let mut broadcasts = Vec::new();

                        let leave_minigame_result = leave_active_minigame_if_any(
                            LeaveMinigameTarget::Single(sender),
                            characters_table_write_handle,
                            minigame_data_write_handle,
                            zones_table_write_handle,
                            None,
                            false,
                            game_server,
                        );
                        match leave_minigame_result {
                            Ok(mut leave_minigame_broadcasts) => broadcasts.append(&mut leave_minigame_broadcasts),
                            Err(err) => info!("Unable to remove player {} from minigame as they were logging out: {}", sender, err),
                        }

                        let Some((character, (_, instance_guid, chunk), ..)) =
                            characters_table_write_handle.remove(player_guid(sender))
                        else {
                            return broadcasts;
                        };

                        let other_players_nearby = ZoneInstance::other_players_nearby(
                            Some(sender),
                            chunk,
                            instance_guid,
                            characters_table_write_handle,
                        );

                        let remove_packets = character
                            .read()
                            .stats
                            .remove_packets(RemovalMode::default());
                        broadcasts.push(Broadcast::Multi(other_players_nearby, remove_packets));

                        clean_up_zone_if_no_players(
                            instance_guid,
                            characters_table_write_handle,
                            zones_table_write_handle,
                        );

                        broadcasts
                    })
                },
            )
        },
    )
}

pub fn send_points_of_interest(game_server: &GameServer) -> Vec<Vec<u8>> {
    let mut points = Vec::new();
    for point_of_interest in game_server.points_of_interest().values() {
        points.push(point_of_interest.into());
    }

    vec![GamePacket::serialize(&TunneledPacket {
        unknown1: true,
        inner: DefinePointsOfInterest { points },
    })]
}
