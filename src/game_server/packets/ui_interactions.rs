use std::io::Cursor;

use packet_serialize::{DeserializePacket, DeserializePacketError};

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
//
// NOTE: clicking an action-bar/consumable slot itself does NOT go through
// this opcode - it produces a Purchase packet (OpCode::Purchase, 0x42) when
// the slot's quantity is 0 (client treats it as "you don't own any, buy
// more?"). See test_data.rs Consumable slot 5 NOTE.
//
// This is parsing only: no response is sent yet. The server-side reaction
// needed to populate flyout panels (Actions/Holoprojectors/Mind
// Tricks/Quick Chat/Recently Used) is still unconfirmed - see task #25.
#[derive(Debug)]
pub struct UiInteraction {
    pub unknown1: u8,
    pub window_name: String,
    pub button_name: String,
    pub param: Option<String>,
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

        Ok(UiInteraction {
            unknown1,
            window_name,
            button_name,
            param,
        })
    }
}
