use std::{collections::HashMap, fs::File, path::Path};

use crate::{
    game_server::{
        packets::{
            chat::{MessagePayload, MessageTypeData, SendMessage},
            housing::{BuildArea, HouseInfo, HouseInstanceData, InnerInstanceData, RoomInstances},
            player_update::{QueueAnimation, RemoveTemporaryModel, UpdateTemporaryModel},
            tunnel::TunneledPacket,
            ui::{ExecuteScriptWithIntParams, ExecuteScriptWithStringParams},
            GamePacket, Name, Pos,
        },
        Broadcast, GameServer, ProcessPacketError, ProcessPacketErrorType,
    },
    ConfigError,
};

use serde::Deserialize;

use super::{
    character::{coerce_to_broadcast_supplier, CharacterType, Player, Role},
    guid::GuidTableIndexer,
    inventory::wield_type_from_inventory,
    lock_enforcer::CharacterLockRequest,
    unique_guid::player_guid,
    zone::{teleport_within_zone, ZoneInstance},
    WriteLockingBroadcastSupplier,
};

/// Emote name -> client animation id (AnimationTypes.xml AnimationSlot id).
///
/// The client's animation tree has two tiers under its "Emotes" branch: a
/// flat set of 23 "core" emotes (ids 3001-3023: Bow, Cry, Drink, FistPump,
/// ForceLift, Laugh, Nod, Point, Threaten, Wave, etc.) with NO AnimationGroup
/// wrapper, and nested sub-branches (Dances, Taunts, Lightsaber Flourishes,
/// Portrait Poses, Handheld Holoprojectors, ids 3101+/3201+/3301+/3401+/3501+)
/// that DO have AnimationGroup wrappers in AnimationGroups.xml.
///
/// Status of the dance/pose/holoprojector sub-branch (confirmed by reading
/// the actual animation clip tables packed into the character .adr files,
/// not just the XML):
/// - Taunts (3201-3210) and Portrait Pose (3401): no usable clip mapping
///   exists on any body or head model (Portrait Pose has a body clip but no
///   head clip anywhere). Genuinely missing/unauthored content; not
///   implementable without new assets, so left out of this table entirely
///   for now.
/// - Dances (3101-3105, the "basic" set): Char_CloneHead.adr has valid
///   mappings for Emo_Dance_Basic1/2/3, Emo_Dance_Bad, and
///   Emo_Dance_RunningMan, with matching .gr2 clips confirmed to exist for
///   both body and head. Fully backed for the clone-headed test character,
///   confirmed working in-game.
/// - Handheld Holoprojectors (3501-3504) and DancePack2/DancePack3/
///   GunganStyle (3106-3111, 3113): asset-complete for the generic
///   Char_HumanMaleHead.adr model, but Char_CloneHead.adr had no entries for
///   them. Patched Char_CloneHead.adr to add these 11 slot entries, pointing
///   at the existing Char_HumanMaleHead_*.gr2 clips (no clone-specific clips
///   exist to author from). Untested in-game until now.
const EMOTES: &[(&str, i32)] = &[
    ("bow", 3001),
    ("charge", 3018),
    ("cry", 3002),
    ("drink", 3013),
    ("fistpump", 3003),
    ("flankleft", 3021),
    ("flankright", 3020),
    ("forcejuggle", 3016),
    ("forcelift", 3014),
    ("forcemeditate", 3017),
    ("forcepush", 3015),
    ("facepalm", 3004),
    ("headshake", 3005),
    ("hitthedeck", 3023),
    ("laugh", 3006),
    ("nod", 3007),
    ("nodyes", 3008),
    ("point", 3009),
    ("retreat", 3022),
    ("rtl", 3019),
    ("threaten", 3010),
    ("tusken", 3012),
    ("wave", 3011),
    // Dances: confirmed clip mappings exist for the clone head model (see
    // notes above). Confirmed working in-game.
    ("dance1", 3101),
    ("dance2", 3102),
    ("dance3", 3103),
    ("dancebad", 3104),
    ("dancerunningman", 3105),
    // DancePack2/DancePack3/GunganStyle: confirmed working in-game.
    // Move1/2/3 clip order is Sprinkler/Shuffle/BeyonceNew, not
    // Diva/Shuffle/Sprinkler as the shop item names misleadingly suggest
    // ("Diva" = the Beyonce-themed clip) - command names match the actual
    // clip played, confirmed by in-game testing.
    ("dancestrut", 3106),
    ("dancecomeback", 3107),
    ("danceshudder", 3108),
    ("dancesprinkler", 3109),
    ("danceshuffle", 3110),
    ("dancediva", 3111),
    ("dancegungan", 3113),
    // Handheld Holoprojectors: just patched into Char_CloneHead.adr (see
    // notes above). Untested in-game until now.
    ("holocody", 3501),
    ("holoyoda", 3502),
    ("holoanakin", 3503),
    ("holomacewindu", 3504),
    // Battle Class Emotes (3602-3613, "Classes" branch in AnimationTypes.xml).
    // These are normally meant to be gated by the player's battle class
    // (Trooper/Jedi/Sith/Merc each get their own 3), but there's no
    // class-check logic in this command dispatcher at all - emotes here are
    // unconditionally available to whoever types the command - so exposing
    // these to every class for now is just a matter of listing them; nothing
    // to bypass. Revisit if/when battle classes get a real restriction system.
    ("trooperacknowledge", 3602),
    ("troopermandown", 3603),
    ("transmissionreceived", 3604),
    ("jediforcepound", 3605),
    ("jedihandstand", 3606),
    ("jedimeditate", 3607),
    ("sithforcelightning", 3608),
    ("sithrage", 3609),
    ("sithseeth", 3610),
    ("mercbarter", 3611),
    ("mercintimidate", 3612),
    ("mercpunch", 3613),
];

fn find_emote(name: &str) -> Option<i32> {
    EMOTES
        .iter()
        .find(|(emote_name, _)| emote_name.eq_ignore_ascii_case(name))
        .map(|(_, id)| *id)
}

/// Holoprojector "disguise" command table: each entry is a non-handheld
/// holoprojector store item (holoprojectors.yaml) that fully replaces the
/// player's visible model via the engine's temporary-model mechanism
/// (Character::temporary_model_id + UpdateTemporaryModel/RemoveTemporaryModel
/// packets - the same already-implemented system used for quest-driven NPC
/// disguises in dialog.rs). The u32 is the model_id resolved from Models.txt
/// (the same ids that were previously stuffed into customizations.yaml as
/// BodyModel customization_param2 values - that approach left the original
/// model rendered idle alongside the new one, since BodyModel customization
/// only swaps cosmetic appearance within the same skeleton and doesn't tear
/// down/respawn the actor the way temporary_model does). The 4 "Handheld
/// Holoprojector" items (Mace Windu/Yoda/Cody/Anakin) are real emotes and
/// live in EMOTES instead, since they don't replace the model.
const DISGUISES: &[(&str, u32)] = &[
    ("disguisesuperbattledroid", 1041),
    ("disguisebattledroid", 1038),
    ("disguisecommandodroid", 1039),
    ("disguisedroideka", 1040),
    ("disguisebith", 1042),
    ("disguisejawa", 1559),
    ("disguisegotal", 1109),
    ("disguiseithorian", 1110),
    ("disguisegungan", 1111),
    ("disguisegeonosian", 1283),
    ("disguisebrainworm", 1284),
    ("disguiseseripas", 1710),
    ("disguiseseripas2", 1710),
    ("disguisemortisdaughter", 1409),
    ("disguisemortisson", 1410),
    ("disguiserancor", 1713),
    ("disguisechewbacca", 1493),
    ("disguiseunderworldithorian", 1494),
    ("disguiseyoda", 1555),
    ("disguisegrievous", 1613),
    ("disguisetalz", 1614),
    ("disguisejabba", 1626),
    ("disguisecadbane", 1627),
    ("disguisegamorreanguard", 1709),
    ("disguisesavageopress", 1714),
    ("disguiseziro", 1820),
    ("disguisekowakian", 1821),
    ("disguiseackbar", 1871),
    ("disguisenossorri", 1872),
    ("disguisekarkarodon", 1873),
    ("disguiseorphne", 1931),
    ("disguisemanchucho", 1578),
    ("disguisemagnaguard", 1939),
    ("disguisesniperdroid", 2165),
    ("disguisegundark", 2334),
    ("disguisec21highsinger", 2340),
    ("disguisedarthmaul", 2339),
    ("disguiseplokoon", 2341),
];

fn find_disguise(name: &str) -> Option<u32> {
    DISGUISES
        .iter()
        .find(|(disguise_name, _)| disguise_name.eq_ignore_ascii_case(name))
        .map(|(_, model_id)| *model_id)
}

/// Weapon Moves Pack I/II command table: each entry is one
/// individually-purchasable "Weapon Move" item (moves.yaml), the
/// FlourishPack it belongs to (1 or 2), and which of the 3 moves in that
/// pack it plays. Resolved against whatever the player currently has
/// equipped via WieldType::flourish_packs() at cast time, NOT baked to a
/// fixed animation id, since the same purchased move plays a different
/// clip depending on weapon class (lightsaber vs. blaster vs. heavy
/// cannon, etc.) - confirmed this is just a matter of checking the
/// equipped weapon's wield type, the same lookup the live equip flow
/// already does for client wield-type packets.
const WEAPON_MOVES: &[(&str, u32, u8, usize)] = &[
    ("weaponmove1", 2237, 1, 0),
    ("weaponmove2", 2238, 1, 1),
    ("weaponmove3", 2239, 1, 2),
    ("weaponmovetaunt", 2797, 2, 0),
    ("weaponmovethrow", 2798, 2, 1),
];

fn find_weapon_move(name: &str) -> Option<(u32, u8, usize)> {
    WEAPON_MOVES
        .iter()
        .find(|(move_name, ..)| move_name.eq_ignore_ascii_case(name))
        .map(|(_, item_guid, pack, move_index)| (*item_guid, *pack, *move_index))
}

/// Looks up the Flourish animation id for the player's currently equipped
/// weapon and the requested pack/move-index, mirroring the same
/// wield_type_from_inventory lookup inventory.rs uses to tell the client
/// what wield type it should display. Returns None if the player has
/// nothing equipped that has any Flourish moves authored for it at all
/// (e.g. no weapon, or Misc/FlameThrower wield types).
fn weapon_move_animation_id(player_stats: &Player, game_server: &GameServer, pack: u8, move_index: usize) -> Option<i32> {
    let equipped_items = player_stats
        .inventory
        .equipped_items(player_stats.inventory.active_battle_class);
    let wield_type = wield_type_from_inventory(&equipped_items, game_server);
    let (pack1, pack2) = wield_type.flourish_packs()?;
    Some(if pack == 1 { pack1[move_index] } else { pack2[move_index] })
}

#[derive(Clone, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct CommandEntry {
    pub description: String,
    pub usage: String,
    pub permission_level: Role,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct CommandConfig {
    pub commands: HashMap<String, CommandEntry>,
}

pub fn load_commands(config_dir: &Path) -> Result<CommandConfig, ConfigError> {
    let mut file = File::open(config_dir.join("commands.yaml"))?;
    let config: CommandConfig = serde_yaml::from_reader(&mut file)?;
    Ok(config)
}

fn server_msg(sender: u32, msg: &str) -> Vec<Broadcast> {
    vec![Broadcast::Single(
        sender,
        vec![
            // Print to chat
            GamePacket::serialize(&TunneledPacket {
                unknown1: true,
                inner: SendMessage {
                    message_type_data: MessageTypeData::World,
                    payload: MessagePayload {
                        sender_guid: 0,
                        target_guid: 0,
                        channel_name: Name::default(),
                        target_name: Name::default(),
                        message: msg.into(),
                        pos: Pos::default(),
                        squad_guid: 0,
                        language_id: 0,
                    },
                },
            }),
            // Print to console
            GamePacket::serialize(&TunneledPacket {
                unknown1: true,
                inner: SendMessage {
                    message_type_data: MessageTypeData::System,
                    payload: MessagePayload {
                        sender_guid: 0,
                        target_guid: 0,
                        channel_name: Name::default(),
                        target_name: Name::default(),
                        message: msg.into(),
                        pos: Pos::default(),
                        squad_guid: 0,
                        language_id: 0,
                    },
                },
            }),
        ],
    )]
}

fn args_len_is_less_than(args: &[String], min_len: usize) -> bool {
    args.len() < min_len
}

pub fn command_details(sender: u32, entry: &CommandEntry) -> Vec<Broadcast> {
    let mut msg = format!(
        "Description: {}\nUsage: {}\n",
        entry.description, entry.usage
    );

    if !entry.notes.is_empty() {
        msg.push_str("Notes:\n");
        for note in entry.notes.iter() {
            msg.push_str(&format!("  - {}\n", note));
        }
    }

    server_msg(sender, &msg)
}

fn command_error(sender: u32, error: &str, info: &CommandEntry) -> Vec<Broadcast> {
    let text = format!("Error: {}\nUsage: {}", error, info.usage);
    server_msg(sender, &text)
}

fn resolve_relative_coord(current_pos: f32, input: &str) -> Result<f32, String> {
    if let Some(offset) = input.strip_prefix('~') {
        if offset.is_empty() {
            Ok(current_pos)
        } else {
            offset
                .parse::<f32>()
                .map(|offset| current_pos + offset)
                .map_err(|_| input.to_string())
        }
    } else {
        input.parse::<f32>().map_err(|_| input.to_string())
    }
}

pub fn process_chat_command(
    sender: u32,
    arguments: &[String],
    game_server: &GameServer,
) -> Result<Vec<Broadcast>, ProcessPacketError> {
    let requester_guid = player_guid(sender);
    let commands_registry = game_server.commands.commands.clone();

    let broadcast_supplier: WriteLockingBroadcastSupplier = game_server
        .lock_enforcer()
        .read_characters(|_| CharacterLockRequest {
            read_guids: Vec::new(),
            write_guids: vec![requester_guid],
            character_consumer: move |characters_table_read_handle, _, mut characters_write, _| {
                let Some(requester_read_handle) = characters_write.get_mut(&requester_guid) else {
                    return coerce_to_broadcast_supplier(|_| Ok(Vec::new()));
                };

                let player_stats = match &mut requester_read_handle.stats.character_type {
                    CharacterType::Player(player) => player.as_mut(),
                    _ => {
                        return coerce_to_broadcast_supplier(move |_| {
                            Err(ProcessPacketError::new(
                                ProcessPacketErrorType::ConstraintViolated,
                                format!(
                                    "Received chat command from {sender} but they were not a player"
                                ),
                            ))
                        });
                    }
                };

                let available_commands: Vec<(&String, &CommandEntry)> = commands_registry
                    .iter()
                    .filter(|(_, entry)| player_stats.role.has_permission(entry.permission_level))
                    .collect();

                let has_any_permission = !available_commands.is_empty();

                let response = {
                    let Some(cmd) = arguments.first().cloned() else {
                        if has_any_permission {
                            return coerce_to_broadcast_supplier(move |_| {
                                Ok(server_msg(sender, "Use ./help for a list of available commands."))
                            });
                        } else {
                            return coerce_to_broadcast_supplier(|_| Ok(Vec::new()));
                        }
                    };

                    let Some(cmd_entry) = commands_registry.get(&cmd) else {
                        if has_any_permission {
                            let msg = format!(
                                "Command {cmd} was not found in the registry. Use ./help for a list of available commands."
                            );
                            return coerce_to_broadcast_supplier(move |_| Ok(server_msg(sender, &msg)));
                        } else {
                            return coerce_to_broadcast_supplier(|_| Ok(Vec::new()));
                        }
                    };

                    if !player_stats.role.has_permission(cmd_entry.permission_level) {
                        return coerce_to_broadcast_supplier(|_| Ok(Vec::new()));
                    }

                    if arguments.iter().any(|arg| arg == "-h" || arg == "--help") {
                        let out = command_details(sender, cmd_entry);
                        return coerce_to_broadcast_supplier(move |_| Ok(out));
                    }

                    let err = move |msg: &str| {
                        let cmd_err = command_error(sender, msg, cmd_entry);
                        coerce_to_broadcast_supplier(move |_| Ok(cmd_err))
                    };

                    match cmd.as_str() {
                        "help" => {
                            let mut msg = "Available commands:\n".to_string();
                            msg.push_str(
                                "Use ./<command> with the help flag (-h or --help) to list command-specific info\n\n",
                            );

                            for (i, (name, entry)) in available_commands.iter().enumerate() {
                                msg.push_str(&format!(
                                    "  ./{} - {}\n    Usage: {}\n",
                                    name, entry.description, entry.usage
                                ));

                                if !entry.notes.is_empty() {
                                    msg.push_str("    Notes:\n");
                                    for note in entry.notes.iter() {
                                        msg.push_str(&format!("      - {}\n", note));
                                    }
                                }

                                if i + 1 < available_commands.len() {
                                    msg.push('\n');
                                }
                            }

                            server_msg(sender, &msg)
                        }

                        "console" => {
                            player_stats.toggles.console = !player_stats.toggles.console;

                            let script = if player_stats.toggles.console {
                                "Console.show"
                            } else {
                                "Console.hide"
                            };

                            vec![Broadcast::Single(
                                sender,
                                vec![GamePacket::serialize(&TunneledPacket {
                                    unknown1: true,
                                    inner: ExecuteScriptWithStringParams {
                                        script_name: script.to_string(),
                                        params: vec![],
                                    },
                                })],
                            )]
                        }

                        "script" => {
                            if args_len_is_less_than(arguments, 2) {
                                return err("No arguments were provided");
                            }

                            let script_name = &arguments[1];
                            let params: Vec<String> =
                                arguments.iter().skip(2).cloned().collect();

                            vec![Broadcast::Single(
                                sender,
                                vec![GamePacket::serialize(&TunneledPacket {
                                    unknown1: true,
                                    inner: ExecuteScriptWithStringParams {
                                        script_name: script_name.to_string(),
                                        params,
                                    },
                                })],
                            )]
                        }

                        "loc" => {
                            let pos = requester_read_handle.stats.pos;
                            let rot = requester_read_handle.stats.rot;

                            let msg = format!(
                                "Position: {}, {}, {}\nRotation: {} {} {}",
                                pos.x, pos.y, pos.z,
                                rot.x, rot.y, rot.z,
                            );

                            server_msg(sender, &msg)
                        }

                        "tp" => {
                            if args_len_is_less_than(arguments, 4) {
                                return err("Not enough arguments provided");
                            }

                            let current_pos = requester_read_handle.stats.pos;

                            let x = match resolve_relative_coord(current_pos.x, &arguments[1]) {
                                Ok(coord) => coord,
                                Err(input) => return err(&format!("Invalid X coordinate: {}", input)),
                            };

                            let y = match resolve_relative_coord(current_pos.y, &arguments[2]) {
                                Ok(coord) => coord,
                                Err(input) => return err(&format!("Invalid Y coordinate: {}", input)),
                            };

                            let z = match resolve_relative_coord(current_pos.z, &arguments[3]) {
                                Ok(coord) => coord,
                                Err(input) => return err(&format!("Invalid Z coordinate: {}", input)),
                            };

                            let destination_pos = Pos { x, y, z, w: current_pos.w };
                            let destination_rot = requester_read_handle.stats.rot;

                            teleport_within_zone(sender, destination_pos, destination_rot)
                        }

                        "clicktp" => {
                            player_stats.toggles.click_to_teleport =
                                !player_stats.toggles.click_to_teleport;
                            vec![Broadcast::Single(sender, vec![])]
                        }

                        "freecam" => {
                            player_stats.toggles.free_camera = !player_stats.toggles.free_camera;
                            make_freecam_packets(sender, requester_guid, player_stats.toggles.free_camera)
                        }

                        // Weapon Moves Pack I/II: each move is an individually
                        // owned item (moves.yaml), and plays a different
                        // Flourish clip depending on whatever weapon class is
                        // currently equipped - resolved live via
                        // weapon_move_animation_id() rather than baked to a
                        // fixed id, since e.g. ./weaponmove1 should play the
                        // lightsaber flourish with a saber equipped but the
                        // HeavyHipGun flourish with a flamethrower equipped.
                        name if find_weapon_move(name).is_some() => {
                            let (required_item_guid, pack, move_index) =
                                find_weapon_move(name).expect("checked by guard above");

                            if !player_stats.inventory.owns_item(required_item_guid) {
                                return err(
                                    "You haven't unlocked this weapon move yet.",
                                );
                            }

                            let Some(animation_id) = weapon_move_animation_id(
                                player_stats,
                                game_server,
                                pack,
                                move_index,
                            ) else {
                                return err(
                                    "Your currently equipped weapon doesn't have any weapon moves.",
                                );
                            };

                            let Some((_, instance_guid, chunk)) =
                                characters_table_read_handle.index1(requester_guid)
                            else {
                                return err("Could not determine your current location");
                            };

                            let mut nearby_player_guids = ZoneInstance::all_players_nearby(
                                chunk,
                                instance_guid,
                                characters_table_read_handle,
                            );
                            if !nearby_player_guids.contains(&sender) {
                                nearby_player_guids.push(sender);
                            }

                            vec![Broadcast::Multi(
                                nearby_player_guids,
                                vec![GamePacket::serialize(&TunneledPacket {
                                    unknown1: true,
                                    inner: QueueAnimation {
                                        character_guid: requester_guid,
                                        animation_id,
                                        queue_pos: 0,
                                        delay_seconds: 0.0,
                                        duration_seconds: 2.0,
                                    },
                                })],
                            )]
                        }

                        "emotes" => {
                            let mut names = EMOTES
                                .iter()
                                .map(|(name, _)| *name)
                                .collect::<Vec<_>>();
                            names.extend(WEAPON_MOVES.iter().map(|(name, ..)| *name));
                            names.extend(DISGUISES.iter().map(|(name, _)| *name));
                            names.push("disguiseoff");
                            server_msg(sender, &format!("Available emotes: {}", names.join(", ")))
                        }

                        "disguiseoff" => {
                            let Some((_, instance_guid, chunk)) =
                                characters_table_read_handle.index1(requester_guid)
                            else {
                                return err("Could not determine your current location");
                            };

                            let nearby_players = ZoneInstance::all_players_nearby(
                                chunk,
                                instance_guid,
                                characters_table_read_handle,
                            );

                            let Some(current_model_id) =
                                requester_read_handle.stats.temporary_model_id.take()
                            else {
                                return err("You aren't disguised.");
                            };

                            vec![Broadcast::Multi(
                                nearby_players,
                                vec![GamePacket::serialize(&TunneledPacket {
                                    unknown1: true,
                                    inner: RemoveTemporaryModel {
                                        guid: requester_guid,
                                        model_id: current_model_id,
                                    },
                                })],
                            )]
                        }

                        // Generic fallback for every holoprojector "disguise" command
                        // registered in commands.yaml (./disguisebattledroid, etc.) -
                        // looked up by name in the DISGUISES table. Uses the engine's
                        // temporary-model mechanism (the same one used for quest-driven
                        // NPC disguises in dialog.rs) to fully replace the player's
                        // visible model rather than layering a cosmetic customization on
                        // top of it, which left the original model rendered idle.
                        name if find_disguise(name).is_some() => {
                            let model_id = find_disguise(name).expect("checked by guard above");

                            let Some((_, instance_guid, chunk)) =
                                characters_table_read_handle.index1(requester_guid)
                            else {
                                return err("Could not determine your current location");
                            };

                            let nearby_players = ZoneInstance::all_players_nearby(
                                chunk,
                                instance_guid,
                                characters_table_read_handle,
                            );

                            let mut packets = Vec::new();
                            if let Some(previous_model_id) =
                                requester_read_handle.stats.temporary_model_id
                            {
                                packets.push(GamePacket::serialize(&TunneledPacket {
                                    unknown1: true,
                                    inner: RemoveTemporaryModel {
                                        guid: requester_guid,
                                        model_id: previous_model_id,
                                    },
                                }));
                            }

                            requester_read_handle.stats.temporary_model_id = Some(model_id);
                            packets.push(GamePacket::serialize(&TunneledPacket {
                                unknown1: true,
                                inner: UpdateTemporaryModel {
                                    model_id,
                                    guid: requester_guid,
                                },
                            }));

                            vec![Broadcast::Multi(nearby_players, packets)]
                        }

                        // Generic fallback for every one-word emote command registered in
                        // commands.yaml (./wave, ./bow, ./laugh, etc.) - looked up by name
                        // in the EMOTES table rather than duplicated per command.
                        name if find_emote(name).is_some() => {
                            let animation_id = find_emote(name).expect("checked by guard above");

                            let Some((_, instance_guid, chunk)) =
                                characters_table_read_handle.index1(requester_guid)
                            else {
                                return err("Could not determine your current location");
                            };

                            let mut nearby_player_guids = ZoneInstance::all_players_nearby(
                                chunk,
                                instance_guid,
                                characters_table_read_handle,
                            );
                            if !nearby_player_guids.contains(&sender) {
                                nearby_player_guids.push(sender);
                            }

                            vec![Broadcast::Multi(
                                nearby_player_guids,
                                vec![GamePacket::serialize(&TunneledPacket {
                                    unknown1: true,
                                    inner: QueueAnimation {
                                        character_guid: requester_guid,
                                        animation_id,
                                        queue_pos: 0,
                                        delay_seconds: 0.0,
                                        duration_seconds: 2.0,
                                    },
                                })],
                            )]
                        }

                        _ => {
                            server_msg(sender, &format!(
                                "Command {cmd} exists in the registry but has no handler."
                            ))
                        }
                    }
                };

                coerce_to_broadcast_supplier(move |_| Ok(response))
            },
        });

    broadcast_supplier?(game_server)
}

fn make_freecam_packets(sender: u32, requester_guid: u64, enabled: bool) -> Vec<Broadcast> {
    if enabled {
        vec![Broadcast::Single(
            sender,
            vec![
                // Enable freecam incase it's disabled so the user doesn't have to open house settings and toggle it manually
                GamePacket::serialize(&TunneledPacket {
                    unknown1: true,
                    inner: ExecuteScriptWithIntParams {
                        script_name: "GameOptions.SetFreeFlyHousingEdit".to_string(),
                        params: vec![1],
                    },
                }),
                // Necessary because build area defines the freecam boundary
                GamePacket::serialize(&TunneledPacket {
                    unknown1: true,
                    inner: HouseInstanceData {
                        inner: InnerInstanceData {
                            house_guid: 0,
                            owner_guid: requester_guid,
                            owner_name: "".to_string(),
                            unknown3: 0,
                            house_name: 0,
                            player_given_name: "".to_string(),
                            unknown4: 0,
                            max_fixtures: 0,
                            unknown6: 0,
                            placed_fixture: vec![],
                            unknown7: false,
                            unknown8: 0,
                            unknown9: 0,
                            unknown10: false,
                            unknown11: 0,
                            unknown12: false,
                            build_areas: vec![BuildArea {
                                min: Pos {
                                    x: f32::MIN,
                                    y: f32::MIN,
                                    z: f32::MIN,
                                    w: 1.0,
                                },
                                max: Pos {
                                    x: f32::MAX,
                                    y: f32::MAX,
                                    z: f32::MAX,
                                    w: 1.0,
                                },
                            }],
                            house_icon: 0,
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
                // Enable edit mode to enter Free Camera
                GamePacket::serialize(&TunneledPacket {
                    unknown1: true,
                    inner: HouseInfo {
                        edit_mode_enabled: true,
                        unknown2: 0,
                        unknown3: true,
                        fixtures: 0,
                        unknown5: 0,
                        unknown6: 0,
                        unknown7: 0,
                    },
                }),
            ],
        )]
    } else {
        vec![Broadcast::Single(
            sender,
            vec![GamePacket::serialize(&TunneledPacket {
                unknown1: true,
                inner: HouseInfo {
                    edit_mode_enabled: false,
                    unknown2: 0,
                    unknown3: false,
                    fixtures: 0,
                    unknown5: 0,
                    unknown6: 0,
                    unknown7: 0,
                },
            })],
        )]
    }
}
