use std::io::{Cursor, Read};

use packet_serialize::DeserializePacket;

use crate::game_server::{
    handlers::{
        character::CharacterType,
        chat_command::{HOLOPROJECTOR_MODELS, MIND_TRICKS, PERSISTENT_MIND_TRICKS},
        guid::GuidTableIndexer,
        lock_enforcer::CharacterLockRequest,
        unique_guid::player_guid,
        zone::ZoneInstance,
    },
    packets::{
        player_update::{
            AddCompositeEffectTag, PlayCompositeEffect, QueueAnimation,
            RemoveCompositeEffectTag, RemoveTemporaryModel, UpdateTemporaryModel,
        },
        tunnel::TunneledPacket,
        GamePacket, Pos,
    },
    Broadcast, GameServer, ProcessPacketError, ProcessPacketErrorType,
};

pub fn process_quick_chat_packet(
    cursor: &mut Cursor<&[u8]>,
    sender: u32,
    game_server: &GameServer,
) -> Result<Vec<Broadcast>, ProcessPacketError> {
    let raw_sub_op: u16 = DeserializePacket::deserialize(cursor).map_err(|e| {
        ProcessPacketError::new(
            ProcessPacketErrorType::ConstraintViolated,
            format!("QuickChat from {sender}: failed to read sub-opcode: {e:?}"),
        )
    })?;

    let mut remaining = Vec::new();
    cursor.read_to_end(&mut remaining).ok();

    match raw_sub_op {
        // sub_op 0x03: action slot activated — client sends quick_chat_id (i32 LE)
        // in bytes 0-3 of remaining.  Look up the QuickChatDefinition by id, then
        // dispatch based on the item:
        //   • one-shot mind tricks  → PlayCompositeEffect sequence + optional animation
        //   • persistent mind tricks → toggle AddCompositeEffectTag/RemoveCompositeEffectTag
        //   • holoprojector disguises → UpdateTemporaryModel
        //   • everything else with animation_id != 0 → QueueAnimation (normal emotes,
        //     handheld holoprojectors)
        //
        // Persistent tricks and disguises mutate character state, so all cases use a
        // write lock on the requester rather than branching between read/write locks.
        0x03 => {
            let quick_chat_id = remaining
                .get(0..4)
                .and_then(|b| b.try_into().ok())
                .map(i32::from_le_bytes)
                .unwrap_or(0);

            let (animation_id, item_id) = game_server
                .quick_chats
                .iter()
                .find(|qc| qc.id == quick_chat_id)
                .map(|qc| (qc.animation_id, qc.item_id))
                .unwrap_or((0, 0));

            let item_guid = item_id as u32;
            let requester_guid = player_guid(sender);

            crate::info!(
                "QuickChat 0x03 from {sender}: quick_chat_id={quick_chat_id} \
                 animation_id={animation_id} item_id={item_id}"
            );

            game_server
                .lock_enforcer()
                .read_characters(|_| CharacterLockRequest {
                    read_guids: Vec::new(),
                    write_guids: vec![requester_guid],
                    character_consumer: move |ctable, _, mut cwrite, _| {
                        let Some((_, instance_guid, chunk)) = ctable.index1(requester_guid) else {
                            return Ok(vec![]);
                        };
                        let mut nearby =
                            ZoneInstance::all_players_nearby(chunk, instance_guid, ctable);
                        if !nearby.contains(&sender) {
                            nearby.push(sender);
                        }

                        // ── One-shot mind trick ──────────────────────────────────────────
                        if let Some(trick) =
                            MIND_TRICKS.iter().find(|e| e.1 == item_guid)
                        {
                            let trick_anim = trick.2;
                            let trick_offset = trick.3;
                            let trick_seq = trick.4;

                            let Some(ch) = cwrite.get_mut(&requester_guid) else {
                                return Ok(vec![]);
                            };

                            let owned = if let CharacterType::Player(p) = &ch.stats.character_type {
                                p.inventory.owns_item(item_guid)
                            } else {
                                false
                            };
                            if !owned {
                                return Ok(vec![]);
                            }

                            let pos = ch.stats.pos;
                            let rot = ch.stats.rot;

                            let (fwd, y_off, right) = trick_offset;
                            let effect_pos = Pos {
                                x: pos.x + rot.x * fwd + rot.z * right,
                                y: pos.y + y_off,
                                z: pos.z + rot.z * fwd - rot.x * right,
                                w: pos.w,
                            };

                            let mut packets: Vec<Vec<u8>> = trick_seq
                                .iter()
                                .map(|(effect_id, delay_ms)| {
                                    GamePacket::serialize(&TunneledPacket {
                                        unknown1: true,
                                        inner: PlayCompositeEffect {
                                            guid: requester_guid,
                                            triggered_by_guid: requester_guid,
                                            composite_effect: *effect_id,
                                            delay_millis: *delay_ms,
                                            duration_millis: 1000,
                                            pos: effect_pos,
                                        },
                                    })
                                })
                                .collect();

                            if let Some(anim_id) = trick_anim {
                                packets.push(GamePacket::serialize(&TunneledPacket {
                                    unknown1: true,
                                    inner: QueueAnimation {
                                        character_guid: requester_guid,
                                        animation_id: anim_id,
                                        queue_pos: 0,
                                        delay_seconds: 0.0,
                                        duration_seconds: 2.0,
                                    },
                                }));
                            }

                            return Ok(vec![Broadcast::Multi(nearby, packets)]);
                        }

                        // ── Persistent mind trick ────────────────────────────────────────
                        if let Some(trick) =
                            PERSISTENT_MIND_TRICKS.iter().find(|e| e.1 == item_guid)
                        {
                            let effect_id = trick.2;
                            let tag_id = trick.3;

                            let Some(ch) = cwrite.get_mut(&requester_guid) else {
                                return Ok(vec![]);
                            };

                            let owned = if let CharacterType::Player(p) = &ch.stats.character_type {
                                p.inventory.owns_item(item_guid)
                            } else {
                                false
                            };
                            if !owned {
                                return Ok(vec![]);
                            }

                            let is_active =
                                ch.stats.composite_effect_tags.contains_key(&tag_id);

                            let packet = if is_active {
                                ch.stats.composite_effect_tags.remove(&tag_id);
                                GamePacket::serialize(&TunneledPacket {
                                    unknown1: true,
                                    inner: RemoveCompositeEffectTag {
                                        guid: requester_guid,
                                        tag_id,
                                    },
                                })
                            } else {
                                ch.stats.composite_effect_tags.insert(tag_id, effect_id);
                                GamePacket::serialize(&TunneledPacket {
                                    unknown1: true,
                                    inner: AddCompositeEffectTag {
                                        guid: requester_guid,
                                        tag_id,
                                        composite_effect_id: effect_id,
                                        triggered_by_guid: 0,
                                        unknown2: 0,
                                    },
                                })
                            };

                            return Ok(vec![Broadcast::Multi(nearby, vec![packet])]);
                        }

                        // ── Holoprojector disguise ───────────────────────────────────────
                        // First click applies the disguise; a second click with the same
                        // holoprojector removes it (toggle behaviour).
                        if let Some(entry) =
                            HOLOPROJECTOR_MODELS.iter().find(|e| e.0 == item_guid)
                        {
                            let model_id = entry.1;

                            let Some(ch) = cwrite.get_mut(&requester_guid) else {
                                return Ok(vec![]);
                            };

                            let owned = if let CharacterType::Player(p) = &ch.stats.character_type {
                                p.inventory.owns_item(item_guid)
                            } else {
                                false
                            };
                            if !owned {
                                return Ok(vec![]);
                            }

                            let mut packets = Vec::new();

                            if ch.stats.temporary_model_id == Some(model_id) {
                                // Same holoprojector clicked again — turn it off.
                                ch.stats.temporary_model_id = None;
                                packets.push(GamePacket::serialize(&TunneledPacket {
                                    unknown1: true,
                                    inner: RemoveTemporaryModel {
                                        guid: requester_guid,
                                        model_id,
                                    },
                                }));
                            } else {
                                // Different or no disguise active — swap to this one.
                                if let Some(prev_model) = ch.stats.temporary_model_id {
                                    packets.push(GamePacket::serialize(&TunneledPacket {
                                        unknown1: true,
                                        inner: RemoveTemporaryModel {
                                            guid: requester_guid,
                                            model_id: prev_model,
                                        },
                                    }));
                                }
                                ch.stats.temporary_model_id = Some(model_id);
                                packets.push(GamePacket::serialize(&TunneledPacket {
                                    unknown1: true,
                                    inner: UpdateTemporaryModel {
                                        model_id,
                                        guid: requester_guid,
                                    },
                                }));
                            }

                            return Ok(vec![Broadcast::Multi(nearby, packets)]);
                        }

                        // ── Normal animation (emotes, handheld holoprojectors) ───────────
                        if animation_id != 0 {
                            return Ok(vec![Broadcast::Multi(
                                nearby,
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
                            )]);
                        }

                        Ok(vec![])
                    },
                })
        }

        // All other sub-opcodes: log and drop.
        _ => {
            crate::info!(
                "QuickChat from {sender}: sub_op=0x{raw_sub_op:02x} remaining={remaining:x?}"
            );
            Ok(vec![])
        }
    }
}
