use std::io::{Cursor, Read};

use packet_serialize::DeserializePacket;

use crate::{
    game_server::{
        packets::{
            housing::{
                BuildArea, EnterRequest, FixtureAssetData, FixtureUpdate, HouseDescription,
                HouseInfo, HouseInstanceData, HouseInstanceEntry, HouseInstanceList, HouseItemList,
                HouseZoneData, HousingOpCode, InnerInstanceData, PlacedFixture,
                RequestPlayerHouseList, RoomInstances, SetEditMode, Unknown1,
            },
            item::{BaseAttachmentGroup, WieldType},
            player_update::{AddNpc, Hostility, Icon, PhysicsState},
            tunnel::TunneledPacket,
            GamePacket, Pos, Target,
        },
        Broadcast, GameServer, ProcessPacketError, ProcessPacketErrorType,
    },
    info, teleport_to_zone,
};

use super::{
    character::{Character, CharacterType, CurrentFixture, PreviousFixture},
    guid::{GuidTableIndexer, IndexedGuid},
    lock_enforcer::{CharacterLockRequest, ZoneLockEnforcer, ZoneLockRequest},
    unique_guid::{
        npc_guid, player_guid, zone_instance_guid, zone_template_guid, FIXTURE_DISCRIMINANT,
    },
    zone::{House, ZoneInstance},
};

/// A house lot: the zone template it opens, the store item that owns it, and
/// what the My Homes list shows.  `image_id` is an Images.txt id (not an icon set).
pub struct HouseLot {
    pub template_guid: u8,
    pub item_guid: u32,
    pub name_id: u32,
    pub name: &'static str,
    pub image_id: u32,
}

const fn lot(template_guid: u8, item_guid: u32, name_id: u32, name: &'static str, image_id: u32) -> HouseLot {
    HouseLot { template_guid, item_guid, name_id, name, image_id }
}

pub const HOUSE_LOTS: &[HouseLot] = &[
    lot(100, 2702, 1987, "Mustafar Lot", 5143),
    lot(101, 2503, 2732, "Kamino Lot", 5141),
    lot(102, 2711, 33632, "Ryloth Lot", 5144),
    lot(103, 2892, 44827, "Iceberg Three Lot", 5140),
    lot(104, 3173, 1991, "Asteroid Lot", 5285),
    lot(105, 3218, 4358, "Umbara Lot", 5363),
    lot(106, 3372, 4607, "Felucia Lot", 5605),
    lot(107, 2236, 32453, "Attack Cruiser", 5145),
    // Lots from the game files the original store never sold (config/items/housing/lots.yaml)
    lot(108, 230200, 52819, "Carlac Lot", 6105),
    lot(109, 230201, 54589, "Dantooine Lot", 7034),
    lot(110, 230202, 4751, "Geonosis Open Lot", 5765),
    lot(111, 230203, 4849, "Maridun Open Lot", 5142),
    lot(112, 230204, 4870, "Orto Plutonia Open Lot", 5889),
    lot(113, 230205, 4880, "Orto Plutonia Holiday Lot", 7058),
    lot(114, 230206, 54526, "Deluxe Ryloth Housing Lot", 6969),
    lot(115, 230207, 53745, "Tatooine Lot", 6423),
    lot(116, 230208, 4825, "Sith Academy Lot", 5826),
    lot(117, 230209, 52687, "Dathomir Sith Temple Lot", 6083),
    lot(118, 230210, 51744, "Felucia Starter Lot", 5931),
    lot(119, 230211, 62227, "Luxury Coruscant Apartment", 6262),
    // Unfinished test lots from the game files
    lot(120, 230212, 62229, "Excis 5-1", 5148),
    lot(121, 230213, 62230, "Ryloth 8-1", 5148),
    // The original Jedi Temple apartments (MemberHousing / NonMemberHousing)
    lot(122, 230214, 62232, "Luxury Padawan Apartment", 5147),
    lot(123, 230215, 62233, "Padawan Apartment", 5148),
];

fn placed_fixture(
    fixture_guid: u64,
    house_guid: u64,
    fixture: &CurrentFixture,
    pos: Pos,
    rot: Pos,
    scale: f32,
) -> PlacedFixture {
    PlacedFixture {
        fixture_guid,
        house_guid,
        fixture_asset_id: fixture.item_def_id,
        unknown2: 0.0,
        pos,
        rot,
        unknown1: Pos {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 0.0,
        },
        npc_guid: fixture_guid,
        item_def_id: fixture.item_def_id,
        unknown3: 0,
        base_attachment_group: BaseAttachmentGroup {
            unknown1: 0,
            unknown2: "".to_string(),
            unknown3: "".to_string(),
            unknown4: 0,
            unknown5: "".to_string(),
        },
        unknown4: "".to_string(),
        unknown5: "".to_string(),
        unknown6: 0,
        unknown7: "".to_string(),
        unknown8: false,
        unknown9: 0,
        scale,
    }
}

#[allow(dead_code)]
fn fixture_item_list(fixtures: &[PreviousFixture], house_guid: u64) -> Vec<u8> {
    let mut unknown1 = Vec::new();
    let mut unknown2 = Vec::new();

    for (index, fixture) in fixtures.iter().enumerate() {
        unknown1.push(Unknown1 {
            fixture_guid: npc_guid(FIXTURE_DISCRIMINANT, house_guid, index as u16),
            item_def_id: fixture.item_def_id,
            unknown1: 0,
            unknown2: vec![],
            unknown3: 0,
            unknown4: 0,
        });
        unknown2.push(FixtureAssetData {
            fixture_asset_id: fixture.item_def_id,
            item_def_id: fixture.item_def_id,
            unknown2: 1,
            model_id: fixture.model_id,
            unknown3: false,
            unknown4: false,
            unknown5: false,
            unknown6: false,
            unknown7: false,
            unknown8: "".to_string(),
            min_scale: 0.5,
            max_scale: 2.0,
            unknown11: 0,
        });
    }

    GamePacket::serialize(&TunneledPacket {
        unknown1: true,
        inner: HouseItemList { unknown1, unknown2 },
    })
}

pub fn fixture_packets(
    house_guid: u64,
    fixture_guid: u64,
    fixture: &CurrentFixture,
    pos: Pos,
    rot: Pos,
    scale: f32,
) -> Vec<Vec<u8>> {
    vec![
        GamePacket::serialize(&TunneledPacket {
            unknown1: true,
            inner: FixtureUpdate {
                placed_fixture: placed_fixture(fixture_guid, house_guid, fixture, pos, rot, scale),
                unknown1: Unknown1 {
                    fixture_guid,
                    item_def_id: fixture.item_def_id,
                    unknown1: 0,
                    unknown2: vec![],
                    unknown3: 0,
                    unknown4: 0,
                },
                unknown2: FixtureAssetData {
                    fixture_asset_id: fixture.item_def_id,
                    item_def_id: fixture.item_def_id,
                    unknown2: 1,
                    model_id: fixture.model_id,
                    unknown3: false,
                    unknown4: false,
                    unknown5: false,
                    unknown6: true,
                    unknown7: false,
                    unknown8: "".to_string(),
                    min_scale: 0.5,
                    max_scale: 2.0,
                    unknown11: 0,
                },
                unknown3: vec![],
                unknown4: 0,
                unknown5: 0,
                unknown6: 0,
            },
        }),
        GamePacket::serialize(&TunneledPacket {
            unknown1: true,
            inner: AddNpc {
                guid: fixture_guid,
                name_id: 0,
                model_id: fixture.model_id,
                unknown3: false,
                chat_text_color: Character::DEFAULT_CHAT_TEXT_COLOR,
                chat_bubble_color: Character::DEFAULT_CHAT_BUBBLE_COLOR,
                chat_scale: 1,
                scale,
                pos,
                rot,
                spawn_animation_id: 1,
                attachments: vec![],
                hostility: Hostility::Neutral,
                unknown10: 0,
                texture_alias: fixture.texture_alias.clone(),
                tint_name: "".to_string(),
                tint_id: 0,
                unknown11: true,
                offset_y: 0.0,
                composite_effect_id: 0,
                wield_type: WieldType::None,
                name_override: "".to_string(),
                hide_name: true,
                name_offset_x: 0.0,
                name_offset_y: 0.0,
                name_offset_z: 0.0,
                terrain_object_id: 0,
                enable_attachments: false,
                speed: 0.0,
                unknown21: false,
                interactable_size_pct: 100,
                walk_animation_id: -1,
                sprint_animation_id: -1,
                stand_animation_id: -1,
                unknown26: true,
                disable_gravity: true,
                sub_title_id: 0,
                one_shot_animation_id: -1,
                temporary_model: 0,
                effects: vec![],
                disable_interact_popup: false,
                unused_death_animation_id: 0,
                unknown34: false,
                show_health: false,
                hide_despawn_fade: false,
                enable_tilt: true,
                base_attachment_group: BaseAttachmentGroup {
                    unknown1: 0,
                    unknown2: "".to_string(),
                    unknown3: "".to_string(),
                    unknown4: 0,
                    unknown5: "".to_string(),
                },
                tilt: Pos {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    w: 0.0,
                },
                unknown40: 0,
                bounce_area_id: -1,
                image_set_id: 0,
                clickable: true,
                rider_guid: 0,
                physics: PhysicsState::default(),
                interact_popup_radius: 0.0,
                target: Target::default(),
                variables: vec![],
                rail_id: 0,
                rail_elapsed_seconds: 0.0,
                rail_offset: Pos {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    w: 0.0,
                },
                unknown54: 0,
                rail_unknown1: 0.0,
                rail_unknown2: 0.0,
                auto_interact_radius: 0.0,
                head_customization_override: "".to_string(),
                hair_customization_override: "".to_string(),
                body_customization_override: "".to_string(),
                override_terrain_model: false,
                hover_glow: 0,
                hover_description: 0,
                fly_over_effect: 0,
                unknown65: 0,
                unknown66: 0,
                unknown67: 0,
                disable_move_to_interact: false,
                unknown69: 0.0,
                unknown70: 0.0,
                unknown71: 0,
                icon_id: Icon::None,
            },
        }),
    ]
}

pub fn prepare_init_house_packets(
    sender: u32,
    zone: &ZoneInstance,
    house: &House,
) -> Result<Vec<Vec<u8>>, ProcessPacketError> {
    if house.is_locked && sender != house.owner {
        return Err(ProcessPacketError::new(
            ProcessPacketErrorType::ConstraintViolated,
            format!(
                "Player {} tried to enter locked house {}",
                sender,
                zone.guid()
            ),
        ));
    }

    Ok(vec![
        GamePacket::serialize(&TunneledPacket {
            unknown1: true,
            inner: HouseZoneData {
                not_editable: sender != house.owner,
                unknown2: 0,
                description: HouseDescription {
                    owner_guid: player_guid(house.owner),
                    house_guid: zone.guid(),
                    house_name: zone.template_name,
                    player_given_name: house.custom_name.clone(),
                    owner_name: house.owner_name.clone(),
                    icon_id: zone.icon,
                    unknown5: false,
                    fixture_count: house.fixtures.len() as u32,
                    unknown7: 0,
                    furniture_score: 0,
                    is_locked: house.is_locked,
                    unknown10: String::new(),
                    unknown11: String::new(),
                    rating: house.rating,
                    total_votes: house.total_votes,
                    is_published: house.is_published,
                    is_rateable: house.is_rateable,
                    unknown16: 0,
                    unknown17: 0,
                },
            },
        }),
        GamePacket::serialize(&TunneledPacket {
            unknown1: true,
            inner: HouseInstanceData {
                inner: InnerInstanceData {
                    house_guid: zone.guid(),
                    owner_guid: player_guid(house.owner),
                    owner_name: house.owner_name.clone(),
                    unknown3: 0,
                    house_name: zone.template_name,
                    player_given_name: house.custom_name.clone(),
                    unknown4: 0,
                    max_fixtures: 10000,
                    unknown6: 0,
                    placed_fixture: vec![],
                    unknown7: false,
                    unknown8: 0,
                    unknown9: 0,
                    unknown10: false,
                    unknown11: 0,
                    unknown12: false,
                    build_areas: house.build_areas.clone(),
                    house_icon: zone.icon,
                    unknown14: false,
                    unknown15: false,
                    unknown16: false,
                    unknown17: 0,
                    unknown18: 0,
                },
                rooms: RoomInstances {
                    unknown1: vec![],
                    unknown2: vec![],
                },
            },
        }),
        GamePacket::serialize(&TunneledPacket {
            unknown1: true,
            inner: HouseInfo {
                edit_mode_enabled: false,
                unknown2: 0,
                unknown3: true,
                fixtures: house.fixtures.len() as u32,
                unknown5: 0,
                unknown6: 0,
                unknown7: 0,
            },
        }),
    ])
}

pub fn process_housing_packet(
    sender: u32,
    game_server: &GameServer,
    cursor: &mut Cursor<&[u8]>,
) -> Result<Vec<Broadcast>, ProcessPacketError> {
    let raw_op_code: u16 = DeserializePacket::deserialize(cursor)?;
    match HousingOpCode::try_from(raw_op_code) {
        Ok(op_code) => match op_code {
            HousingOpCode::SetEditMode => {
                let set_edit_mode: SetEditMode = DeserializePacket::deserialize(cursor)?;
                game_server.lock_enforcer().read_characters(|_| CharacterLockRequest {
                    read_guids: Vec::new(),
                    write_guids: Vec::new(),
                    character_consumer: |characters_table_read_handle, _, _, minigame_data_lock_enforcer| {
                        let Some((_, instance_guid, _)) = characters_table_read_handle.index1(player_guid(sender)) else {
                            return Err(ProcessPacketError::new(ProcessPacketErrorType::ConstraintViolated, format!("Non-existent player {sender} tried to set edit mode")));
                        };

                        let zones_lock_enforcer: ZoneLockEnforcer = minigame_data_lock_enforcer.into();
                        let packets = zones_lock_enforcer.read_zones(|_| ZoneLockRequest {
                            read_guids: vec![instance_guid],
                            write_guids: Vec::new(),
                            zone_consumer: |_, zones_read, _| {
                                let Some(zone_read_handle) = zones_read.get(&instance_guid) else {
                                    return Err(ProcessPacketError::new(ProcessPacketErrorType::ConstraintViolated, format!(
                                        "Player {sender} tried to set edit mode but is not in any zone"
                                    )));
                                };

                                let Some(house) = &zone_read_handle.house_data else {
                                    return Err(ProcessPacketError::new(ProcessPacketErrorType::ConstraintViolated, format!(
                                        "Player {sender} tried to set edit mode outside of a house"
                                    )));
                                };

                                if house.owner != sender {
                                    return Err(ProcessPacketError::new(ProcessPacketErrorType::ConstraintViolated, format!(
                                        "Player {sender} tried to set edit mode in a house they don't own"
                                    )));
                                }

                                Ok(vec![GamePacket::serialize(&TunneledPacket {
                                    unknown1: true,
                                    inner: HouseInfo {
                                        edit_mode_enabled: set_edit_mode.enabled,
                                        unknown2: 0,
                                        unknown3: true,
                                        fixtures: house.fixtures.len() as u32,
                                        unknown5: 0,
                                        unknown6: 0,
                                        unknown7: 0,
                                    },
                                })])
                            },
                        })?;

                        Ok(vec![Broadcast::Single(sender, packets)])
                    },
                })
            }
            HousingOpCode::RequestPlayerHouseList => {
                let request = RequestPlayerHouseList::deserialize(cursor)?;
                info!(
                    "RequestPlayerHouseList from {}: player_guid={}",
                    sender, request.player_guid
                );

                // Lots the requested player has bought (empty if they aren't online).
                let owned: Vec<&HouseLot> = game_server.lock_enforcer().read_characters(|_| {
                    CharacterLockRequest {
                        read_guids: vec![request.player_guid],
                        write_guids: Vec::new(),
                        character_consumer: |_, characters_read, _, _| {
                            let purchased = characters_read
                                .get(&request.player_guid)
                                .and_then(|character| match &character.stats.character_type {
                                    CharacterType::Player(player) => Some(&player.purchased_items),
                                    _ => None,
                                });
                            HOUSE_LOTS
                                .iter()
                                .filter(|lot| purchased.is_some_and(|items| items.contains(&lot.item_guid)))
                                .collect()
                        },
                    }
                });

                let instances = owned
                    .into_iter()
                    .map(|lot| HouseInstanceEntry {
                        description: HouseDescription {
                            owner_guid: request.player_guid,
                            house_guid: owned_house_guid(request.player_guid, lot.template_guid),
                            house_name: lot.name_id,
                            player_given_name: lot.name.to_string(),
                            owner_name: String::new(),
                            icon_id: lot.image_id,
                            unknown5: true,
                            fixture_count: 0,
                            unknown7: 0,
                            furniture_score: 0,
                            is_locked: false,
                            unknown10: String::new(),
                            unknown11: String::new(),
                            rating: 0.0,
                            total_votes: 0,
                            is_published: false,
                            is_rateable: false,
                            unknown16: 0,
                            unknown17: 0,
                        },
                    })
                    .collect();

                Ok(vec![Broadcast::Single(
                    sender,
                    vec![GamePacket::serialize(&TunneledPacket {
                        unknown1: true,
                        inner: HouseInstanceList {
                            instances,
                            owner_guid: request.player_guid,
                        },
                    })],
                )])
            }
            HousingOpCode::ExitHouse => {
                // House_Dock "Exit House" button: send the player back where they
                // were before entering (house zones don't overwrite previous_location).
                game_server.lock_enforcer().write_characters(
                    |characters_table_write_handle, minigame_data_lock_enforcer| {
                        let previous_location = characters_table_write_handle
                            .get(player_guid(sender))
                            .and_then(|character| match &character.read().stats.character_type {
                                CharacterType::Player(player) => Some(player.previous_location.clone()),
                                _ => None,
                            })
                            .ok_or_else(|| {
                                ProcessPacketError::new(
                                    ProcessPacketErrorType::ConstraintViolated,
                                    format!("Unknown player {sender} tried to exit a house"),
                                )
                            })?;

                        let zones_lock_enforcer: ZoneLockEnforcer<'_> =
                            minigame_data_lock_enforcer.into();
                        zones_lock_enforcer.write_zones(|zones_table_write_handle| {
                            let instance_guid = game_server.get_or_create_instance(
                                characters_table_write_handle,
                                zones_table_write_handle,
                                previous_location.template_guid,
                                1,
                            )?;
                            let Some(zone_read_handle) = zones_table_write_handle.get(instance_guid) else {
                                return Err(ProcessPacketError::new(
                                    ProcessPacketErrorType::ConstraintViolated,
                                    format!("Zone instance {instance_guid} is missing after creation"),
                                ));
                            };
                            teleport_to_zone!(
                                characters_table_write_handle,
                                sender,
                                zones_table_write_handle,
                                &zone_read_handle.read(),
                                Some(previous_location.pos),
                                Some(previous_location.rot),
                                game_server.mounts(),
                            )
                        })
                    },
                )
            }
            HousingOpCode::EnterRequest => {
                let enter_request: EnterRequest = DeserializePacket::deserialize(cursor)?;

                game_server.lock_enforcer().write_characters(
                    |characters_table_write_handle, minigame_data_lock_enforcer| {
                        let zones_lock_enforcer: ZoneLockEnforcer<'_> =
                            minigame_data_lock_enforcer.into();
                        zones_lock_enforcer.write_zones(|zones_table_write_handle| {
                            // Create the house instance if it does not already exist
                            if zones_table_write_handle
                                .get(enter_request.house_guid)
                                .is_none()
                            {
                                let template_guid = zone_template_guid(enter_request.house_guid);
                                let Some(template) =
                                    game_server.read_zone_templates().get(&template_guid)
                                else {
                                    return Err(ProcessPacketError::new(
                                        ProcessPacketErrorType::ConstraintViolated,
                                        format!(
                                            "Tried to enter house with unknown template {template_guid}"
                                        ),
                                    ));
                                };

                                zones_table_write_handle.insert(ZoneInstance::new_house(
                                    enter_request.house_guid,
                                    template,
                                    lookup_house(sender, enter_request.house_guid)?,
                                    characters_table_write_handle,
                                ));
                            }

                            let Some(zone_read_handle) =
                                zones_table_write_handle.get(enter_request.house_guid)
                            else {
                                return Err(ProcessPacketError::new(
                                    ProcessPacketErrorType::ConstraintViolated,
                                    format!("Unable to create house {}", enter_request.house_guid),
                                ));
                            };

                            teleport_to_zone!(
                                characters_table_write_handle,
                                sender,
                                zones_table_write_handle,
                                &zone_read_handle.read(),
                                None,
                                None,
                                game_server.mounts(),
                            )
                        })
                    },
                )
            }
            _ => {
                let mut buffer = Vec::new();
                cursor.read_to_end(&mut buffer)?;
                Err(ProcessPacketError::new(
                    ProcessPacketErrorType::UnknownOpCode,
                    format!("Unimplemented housing packet: {op_code:?}, {buffer:x?}"),
                ))
            }
        },
        Err(_) => {
            let mut buffer = Vec::new();
            cursor.read_to_end(&mut buffer)?;
            Err(ProcessPacketError::new(
                ProcessPacketErrorType::UnknownOpCode,
                format!("Unknown housing packet: {raw_op_code}, {buffer:x?}"),
            ))
        }
    }
}

/// Per-zone build area bounds derived from gzne binary analysis.
fn build_areas_for_template(template_guid: u8) -> Vec<BuildArea> {
    match template_guid {
        // Mustafar (100) — confirmed in-game
        100 => vec![BuildArea {
            min: Pos { x: 384.0, y: -1.0, z: 448.0, w: 0.0 },
            max: Pos { x: 512.0, y: 100.0, z: 512.0, w: 0.0 },
        }],
        // Kamino (101) — estimated from gzne vertex data
        101 => vec![BuildArea {
            min: Pos { x: 512.0, y: 10.0, z: 384.0, w: 0.0 },
            max: Pos { x: 640.0, y: 110.0, z: 448.0, w: 0.0 },
        }],
        // Ryloth (102) — same footprint as Mustafar (confirmed identical gzne coordinate range)
        102 => vec![BuildArea {
            min: Pos { x: 384.0, y: -1.0, z: 448.0, w: 0.0 },
            max: Pos { x: 512.0, y: 100.0, z: 512.0, w: 0.0 },
        }],
        // Iceberg Three (103) — estimated from gzne vertex data (y floor ~47)
        103 => vec![BuildArea {
            min: Pos { x: 480.0, y: 47.0, z: 448.0, w: 0.0 },
            max: Pos { x: 640.0, y: 148.0, z: 512.0, w: 0.0 },
        }],
        // Asteroid (104) — placeholder (gzne vertex data not cleanly extracted)
        104 => vec![BuildArea {
            min: Pos { x: 384.0, y: -1.0, z: 448.0, w: 0.0 },
            max: Pos { x: 512.0, y: 111.0, z: 512.0, w: 0.0 },
        }],
        // Umbara (105) — estimated from hex decode of gzne (x=307-448, y=10-110, z=307-448)
        105 => vec![BuildArea {
            min: Pos { x: 307.0, y: 10.0, z: 307.0, w: 0.0 },
            max: Pos { x: 448.0, y: 110.0, z: 448.0, w: 0.0 },
        }],
        // Felucia (106) — confirmed from gzne vertex data (x=180-320, y=12-212, z=180-320)
        106 => vec![BuildArea {
            min: Pos { x: 180.0, y: 12.0, z: 180.0, w: 0.0 },
            max: Pos { x: 320.0, y: 212.0, z: 320.0, w: 0.0 },
        }],
        // Starship/Attack Cruiser (107) — from gzne (x~64-96, y~20-120, z~-3220 to -3180)
        107 => vec![BuildArea {
            min: Pos { x: 64.0, y: 20.0, z: -3220.0, w: 0.0 },
            max: Pos { x: 96.0, y: 120.0, z: -3180.0, w: 0.0 },
        }],
        // Unknown template — fall back to Mustafar dimensions
        _ => vec![BuildArea {
            min: Pos { x: 384.0, y: -1.0, z: 448.0, w: 0.0 },
            max: Pos { x: 512.0, y: 100.0, z: 512.0, w: 0.0 },
        }],
    }
}

/// House instances carry their owner in the zone index (high bit set), so every
/// player gets their own copy of each lot they own.
const HOUSE_OWNER_FLAG: u32 = 1 << 31;

pub fn owned_house_guid(owner_guid: u64, template_guid: u8) -> u64 {
    zone_instance_guid(HOUSE_OWNER_FLAG | (owner_guid as u32 & !HOUSE_OWNER_FLAG), template_guid)
}

pub fn house_owner(house_guid: u64) -> Option<u32> {
    let index = super::unique_guid::shorten_zone_index(house_guid);
    (index & HOUSE_OWNER_FLAG != 0).then_some(index & !HOUSE_OWNER_FLAG)
}

/// Whether the owner of the house `zone_instance` is (or, outside a house, the
/// requester) has bought `item`.  Visitors use the owner's saved profile.
pub fn house_owner_has_item(
    zone_instance: &ZoneInstance,
    requester: u32,
    requester_items: &std::collections::HashSet<u32>,
    item: u32,
    game_server: &GameServer,
) -> bool {
    match zone_instance.house_data.as_ref().map(|house| house.owner) {
        Some(owner) if owner != requester => {
            super::profile::load_profile(game_server.profiles_dir(), owner)
                .purchased_items
                .contains(&item)
        }
        _ => requester_items.contains(&item),
    }
}

pub fn lookup_house(sender: u32, house_guid: u64) -> Result<House, ProcessPacketError> {
    let template_guid = super::unique_guid::zone_template_guid(house_guid);
    let owner = house_owner(house_guid).unwrap_or(sender);
    info!("Found house {} (template {}, owner {})", house_guid, template_guid, owner);
    Ok(House {
        owner,
        owner_name: "BLASTER NICESHOt".to_string(),
        custom_name: "Blaster's Test Lot".to_string(),
        rating: 3.5,
        total_votes: 100,
        fixtures: if template_guid == 100 { vec![
            PreviousFixture {
                pos: Pos {
                    x: 495.0,
                    y: 0.0,
                    z: 481.5,
                    w: 1.0,
                },
                rot: Pos {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    w: 0.0,
                },
                scale: 1.0,
                item_def_id: 1389,
                model_id: 1417,
                texture_alias: "".to_string(),
            },
            PreviousFixture {
                pos: Pos {
                    x: 495.0,
                    y: 0.0,
                    z: 483.5,
                    w: 1.0,
                },
                rot: Pos {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    w: 0.0,
                },
                scale: 1.0,
                item_def_id: 1391,
                model_id: 1419,
                texture_alias: "".to_string(),
            },
            PreviousFixture {
                pos: Pos {
                    x: 495.0,
                    y: 0.0,
                    z: 485.5,
                    w: 1.0,
                },
                rot: Pos {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    w: 0.0,
                },
                scale: 1.0,
                item_def_id: 1391,
                model_id: 1419,
                texture_alias: "".to_string(),
            },
            PreviousFixture {
                pos: Pos {
                    x: 495.0,
                    y: 0.0,
                    z: 487.5,
                    w: 1.0,
                },
                rot: Pos {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    w: 0.0,
                },
                scale: 1.0,
                item_def_id: 1392,
                model_id: 1420,
                texture_alias: "".to_string(),
            },
            PreviousFixture {
                pos: Pos {
                    x: 495.0,
                    y: 0.0,
                    z: 475.5,
                    w: 1.0,
                },
                rot: Pos {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    w: 0.0,
                },
                scale: 1.0,
                item_def_id: 1390,
                model_id: 1418,
                texture_alias: "".to_string(),
            },
            PreviousFixture {
                pos: Pos {
                    x: 495.0,
                    y: 0.5,
                    z: 475.5,
                    w: 1.0,
                },
                rot: Pos {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    w: 0.0,
                },
                scale: 1.0,
                item_def_id: 1390,
                model_id: 1418,
                texture_alias: "".to_string(),
            },
            PreviousFixture {
                pos: Pos {
                    x: 495.0,
                    y: 1.0,
                    z: 475.5,
                    w: 1.0,
                },
                rot: Pos {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    w: 0.0,
                },
                scale: 1.0,
                item_def_id: 1390,
                model_id: 1418,
                texture_alias: "".to_string(),
            },
            PreviousFixture {
                pos: Pos {
                    x: 495.0,
                    y: 1.5,
                    z: 475.5,
                    w: 1.0,
                },
                rot: Pos {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    w: 0.0,
                },
                scale: 1.0,
                item_def_id: 1390,
                model_id: 1418,
                texture_alias: "".to_string(),
            },
            PreviousFixture {
                pos: Pos {
                    x: 490.0,
                    y: 0.0,
                    z: 475.5,
                    w: 1.0,
                },
                rot: Pos {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    w: 0.0,
                },
                scale: 1.0,
                item_def_id: 1388,
                model_id: 1416,
                texture_alias: "".to_string(),
            },
            PreviousFixture {
                pos: Pos {
                    x: 490.0,
                    y: 0.5,
                    z: 475.5,
                    w: 1.0,
                },
                rot: Pos {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    w: 0.0,
                },
                scale: 1.0,
                item_def_id: 1388,
                model_id: 1416,
                texture_alias: "".to_string(),
            },
            PreviousFixture {
                pos: Pos {
                    x: 490.0,
                    y: 1.0,
                    z: 475.5,
                    w: 1.0,
                },
                rot: Pos {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    w: 0.0,
                },
                scale: 1.0,
                item_def_id: 1388,
                model_id: 1416,
                texture_alias: "".to_string(),
            },
            PreviousFixture {
                pos: Pos {
                    x: 490.0,
                    y: 1.5,
                    z: 475.5,
                    w: 1.0,
                },
                rot: Pos {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    w: 0.0,
                },
                scale: 1.0,
                item_def_id: 1388,
                model_id: 1416,
                texture_alias: "".to_string(),
            },
        ] } else { vec![] },
        build_areas: build_areas_for_template(template_guid),
        is_locked: false,
        is_published: false,
        is_rateable: false,
    })
}
