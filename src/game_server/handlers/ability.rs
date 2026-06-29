use std::{
    collections::HashMap,
    fs::File,
    io::{Cursor, Read},
    path::Path,
};

use packet_serialize::DeserializePacket;
use serde::Deserialize;

use crate::{
    game_server::{
        handlers::{
            character::CharacterType, guid::GuidTableIndexer, lock_enforcer::CharacterLockRequest,
            unique_guid::player_guid, zone::ZoneInstance,
        },
        packets::{
            ability::{experimental_ability_definition, AbilityOpCode, RequestStartCast},
            player_update::QueueAnimation,
            tunnel::TunneledPacket,
            AbilitySubType, GamePacket,
        },
        Broadcast, GameServer, ProcessPacketError, ProcessPacketErrorType,
    },
    ConfigError,
};

const fn default_ability_sub_type() -> AbilitySubType {
    AbilitySubType::InstantSingleTarget
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbilityConfig {
    pub icon_set_id: u32,
    pub name_id: u32,
    #[serde(default)]
    pub required_force_points: u32,
    #[serde(default)]
    pub use_cooldown_millis: u32,
    #[serde(default)]
    pub init_cooldown_millis: u32,
    #[serde(default)]
    pub area_of_effect_radius: f32,
    #[serde(default)]
    pub max_distance_from_player: f32,
    #[serde(default = "default_ability_sub_type")]
    pub ability_sub_type: AbilitySubType,
    /// Client AnimationSlot id to play (via QueueAnimation) when this ability
    /// is cast. CONFIRMED mechanism (see ./test913, which plays animation 913
    /// with its effects correctly). 0 means no animation is played.
    #[serde(default)]
    pub animation_id: i32,
}

pub fn load_abilities(config_dir: &Path) -> Result<HashMap<String, AbilityConfig>, ConfigError> {
    let file = File::open(config_dir.join("abilities.yaml"))?;
    let abilities: HashMap<String, AbilityConfig> = serde_yaml::from_reader(file)?;

    Ok(abilities)
}

pub fn process_ability(
    game_server: &GameServer,
    sender: u32,
    cursor: &mut Cursor<&[u8]>,
) -> Result<Vec<Broadcast>, ProcessPacketError> {
    let raw_op_code: u16 = DeserializePacket::deserialize(cursor)?;
    crate::debug!(
        "Ability raw payload: raw_op_code={raw_op_code} (0x{raw_op_code:x}), decoded={:?}, remaining_bytes={:x?}",
        AbilityOpCode::try_from(raw_op_code).ok(),
        &cursor.get_ref()[cursor.position() as usize..]
    );
    match AbilityOpCode::try_from(raw_op_code) {
        Ok(op_code) => match op_code {
            // CONFIRMED via live capture: see comment on
            // AbilityDefinitionResponse in packets/ability.rs - the
            // response byte layout sent below matched a real captured
            // server reply byte-for-byte. The request's bare u32 (weapon
            // action-bar slot index 0-3) and its use to resolve the ability
            // below remain the original best-effort interpretation and
            // haven't independently been re-confirmed against a captured
            // C->S request payload.
            AbilityOpCode::RequestDefinition => {
                let requested_slot: u32 = DeserializePacket::deserialize(cursor)?;

                game_server
                    .lock_enforcer()
                    .read_characters(|_| CharacterLockRequest {
                        read_guids: vec![],
                        write_guids: vec![player_guid(sender)],
                        character_consumer: |_, _, mut characters_write, _| {
                            let Some(character_write_handle) =
                                characters_write.get_mut(&player_guid(sender))
                            else {
                                return Ok(Vec::new());
                            };

                            let CharacterType::Player(ref player) =
                                character_write_handle.stats.character_type
                            else {
                                return Ok(Vec::new());
                            };

                            let mut groups = player.action_bar.weapon_abilities.clone();
                            groups.sort_by_key(|group| group.priority);

                            let resolved_ability = groups
                                .iter()
                                .flat_map(|group| group.ability_keys.iter())
                                .filter_map(|key| game_server.abilities().get(key))
                                .nth(requested_slot as usize);

                            let Some(ability) = resolved_ability else {
                                crate::debug!(
                                    "RequestDefinition: no weapon ability resolved for sender {sender} slot {requested_slot}; sending nothing"
                                );
                                return Ok(Vec::new());
                            };

                            let response = experimental_ability_definition(
                                requested_slot,
                                ability.icon_set_id,
                                ability.name_id,
                                ability.required_force_points,
                            );

                            Ok(vec![Broadcast::Single(
                                sender,
                                vec![GamePacket::serialize(&TunneledPacket {
                                    unknown1: true,
                                    inner: response,
                                })],
                            )])
                        },
                    })
            }
            // CONFIRMED via live capture: action_bar_type=Weapon(1), slot_index
            // matches the action bar slot (e.g. 3 for the slot labeled "Action 4"
            // where Spring Fever's ability sits), target is Guid{self, 0} when
            // self-cast. We use slot_index the same way RequestDefinition does,
            // to resolve which weapon ability was cast, then play its
            // configured animation_id via the same QueueAnimation broadcast
            // confirmed working by ./test913.
            AbilityOpCode::RequestStartCast => {
                let request: RequestStartCast = DeserializePacket::deserialize(cursor)?;

                game_server
                    .lock_enforcer()
                    .read_characters(|_| CharacterLockRequest {
                        read_guids: vec![],
                        write_guids: vec![player_guid(sender)],
                        character_consumer: |characters_table_read_handle, _, mut characters_write, _| {
                            let Some(character_write_handle) =
                                characters_write.get_mut(&player_guid(sender))
                            else {
                                return Ok(Vec::new());
                            };

                            let CharacterType::Player(ref player) =
                                character_write_handle.stats.character_type
                            else {
                                return Ok(Vec::new());
                            };

                            let mut groups = player.action_bar.weapon_abilities.clone();
                            groups.sort_by_key(|group| group.priority);

                            let resolved_ability = groups
                                .iter()
                                .flat_map(|group| group.ability_keys.iter())
                                .filter_map(|key| game_server.abilities().get(key))
                                .nth(request.slot_index as usize);

                            let Some(ability) = resolved_ability else {
                                crate::debug!(
                                    "RequestStartCast: no weapon ability resolved for sender {sender} slot {}; not animating",
                                    request.slot_index
                                );
                                return Ok(Vec::new());
                            };

                            if ability.animation_id == 0 {
                                return Ok(Vec::new());
                            }

                            let Some((_, instance_guid, chunk)) =
                                characters_table_read_handle.index1(player_guid(sender))
                            else {
                                return Ok(Vec::new());
                            };

                            let mut nearby_player_guids = ZoneInstance::all_players_nearby(
                                chunk,
                                instance_guid,
                                characters_table_read_handle,
                            );
                            if !nearby_player_guids.contains(&sender) {
                                nearby_player_guids.push(sender);
                            }

                            Ok(vec![Broadcast::Multi(
                                nearby_player_guids,
                                vec![GamePacket::serialize(&TunneledPacket {
                                    unknown1: true,
                                    inner: QueueAnimation {
                                        character_guid: player_guid(sender),
                                        animation_id: ability.animation_id,
                                        queue_pos: 0,
                                        delay_seconds: 0.0,
                                        duration_seconds: 2.0,
                                    },
                                })],
                            )])
                        },
                    })
            }
            _ => {
                let mut buffer = Vec::new();
                cursor.read_to_end(&mut buffer)?;
                Err(ProcessPacketError::new(
                    ProcessPacketErrorType::UnknownOpCode,
                    format!("Unimplemented ability packet: {op_code:?}, {buffer:x?}"),
                ))
            }
        },
        Err(_) => {
            let mut buffer = Vec::new();
            cursor.read_to_end(&mut buffer)?;
            Err(ProcessPacketError::new(
                ProcessPacketErrorType::UnknownOpCode,
                format!("Unknown ability packet: {raw_op_code}, {buffer:x?}"),
            ))
        }
    }
}
