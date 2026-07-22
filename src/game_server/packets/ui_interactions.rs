use std::io::{Cursor, Read};

use packet_serialize::{DeserializePacket, DeserializePacketError};

/// One entry in a SetFavoritesPacket (sub-opcode 2).
/// Observed format: quick_chat_id (i32 LE) + 4 zero bytes.
#[derive(Debug)]
pub struct SetFavoriteEntry {
    pub quick_chat_id: i32,
    pub unknown: i32, // always 0 in observed captures
}

/// Sent by the client (opcode 0xbd, first byte = 2) when the player stars
/// an emote in the Actions menu.  A single-entry packet fires on each star
/// click; a bulk packet (all entries identical) fires when the menu closes.
/// The server stores these in player.fav_emotes (max 4, newest-first FIFO).
#[derive(Debug)]
pub struct SetFavoritesPacket {
    pub sub_opcode: u8,
    pub entries: Vec<SetFavoriteEntry>,
}

impl DeserializePacket for SetFavoritesPacket {
    fn deserialize(cursor: &mut Cursor<&[u8]>) -> Result<Self, DeserializePacketError> {
        let sub_opcode: u8 = DeserializePacket::deserialize(cursor)?;
        let count: u32 = DeserializePacket::deserialize(cursor)?;
        let mut entries = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let quick_chat_id: i32 = DeserializePacket::deserialize(cursor)?;
            let unknown: i32 = DeserializePacket::deserialize(cursor)?;
            entries.push(SetFavoriteEntry {
                quick_chat_id,
                unknown,
            });
        }
        Ok(SetFavoritesPacket {
            sub_opcode,
            entries,
        })
    }
}

// CONFIRMED via live packet capture (2026-06-29, slot-5 holoprojector
// rendering test session): clicking a UI element sends OpCode::UiInteractions
// (0xbd) with this layout:
//   unknown1: u8 - constant 4 across every captured interaction
//     (chat-channel switch, opening Actions menu, exit-client confirmation);
//     previously misdeclared as u32, which devoured 3 extra bytes belonging
//     to the following length prefix and caused every single packet to fail
//     to parse with UnexpectedEof.
//   window_name: String (4-byte LE length prefix + raw bytes, no null
//     terminator - standard packet_serialize String)
//   button_name: String
//   param: String, present only when the button click carries an extra
//     argument (e.g. ClickGotoItemFromActionsButton's "nonCombat"); absent
//     for simpler clicks like ClickActionsButton. There's no length/flag
//     field indicating presence - we just check if any bytes remain.
//   trailing: any bytes left after the param string. Captured verbatim so
//     handlers can inspect them — some interactions may carry extra fields
//     (e.g. a quick_chat_id i32 after the param for ClickFavActionButton).
//
// NOTE: clicking an action-bar/consumable slot itself does NOT go through
// this opcode - it produces a Purchase packet (OpCode::Purchase, 0x42) when
// the slot's quantity is 0 (client treats it as "you don't own any, buy
// more?"). See test_data.rs Consumable slot 5 NOTE.
#[derive(Debug)]
pub struct UiInteraction {
    pub unknown1: u8,
    pub window_name: String,
    pub button_name: String,
    pub param: Option<String>,
    /// Any bytes remaining in the packet after the param string.
    /// Non-empty bytes here indicate extra fields we haven't named yet.
    pub trailing: Vec<u8>,
}

impl DeserializePacket for UiInteraction {
    fn deserialize(cursor: &mut Cursor<&[u8]>) -> Result<Self, DeserializePacketError> {
        let unknown1: u8 = DeserializePacket::deserialize(cursor)?;
        let window_name: String = DeserializePacket::deserialize(cursor)?;
        let button_name: String = DeserializePacket::deserialize(cursor)?;

        let remaining = cursor.get_ref().len() as u64 - cursor.position();
        let param = if remaining > 0 {
            Some(String::deserialize(cursor)?)
        } else {
            None
        };

        let mut trailing = Vec::new();
        cursor.read_to_end(&mut trailing).ok();

        Ok(UiInteraction {
            unknown1,
            window_name,
            button_name,
            param,
            trailing,
        })
    }
}
