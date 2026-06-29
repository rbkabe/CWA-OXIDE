use num_enum::TryFromPrimitive;
use packet_serialize::SerializePacket;

use super::{GamePacket, OpCode};

/// Sub-opcodes under `OpCode::Store` (0xa4).
///
/// Confirmed via Ghidra analysis of the client's Store packet dispatcher
/// (`FUN_00b24dc0`, reached via `BaseClient::vfunction25_for_ClientGatewayHandler`
/// case 0xa4 -> `FUN_0081ae00` -> `FUN_00b24dc0`): the client constructs a
/// `BaseCoinStorePacket` and can RECEIVE sub-opcodes
/// {1, 3, 6, 7, 9, 10, 11, 13, 17 (0x11), 18 (0x12)}. Sub-opcode 8 does not
/// appear in that list -- it's client -> server only (confirmed via live
/// capture: the client sends an empty `[a4, 0, 8, 0]` packet, presumably when
/// opening the Gear/Store tab), and is not yet confirmed which response(s) it
/// expects in return. `ItemList` is the best first guess since it's already
/// built elsewhere (`StoreItemList::from(&self.costs)`, sent unconditionally
/// at login) and is the simplest "give me what's in the store" response.
#[derive(Copy, Clone, Debug, TryFromPrimitive)]
#[repr(u16)]
pub enum StoreOpCode {
    ItemList = 0x1,
    ItemDefinitionsReply = 0x3,
    /// Client -> server only; not part of the client's receive dispatch table.
    RequestItemList = 0x8,
}

impl SerializePacket for StoreOpCode {
    fn serialize(&self, buffer: &mut Vec<u8>) {
        OpCode::Store.serialize(buffer);
        (*self as u16).serialize(buffer);
    }
}

pub struct StoreItem {
    pub guid: u32,
    pub unknown2: u32,
    pub unknown3: u32,
    pub unknown4: bool,
    pub unknown5: bool,
    pub unknown6: u32,
    pub unknown7: bool,
    pub unknown8: bool,
    pub base_cost: u32,
    pub unknown10: u32,
    pub unknown11: u32,
    pub unknown12: u32,
    pub member_cost: u32,
}

impl SerializePacket for StoreItem {
    fn serialize(&self, buffer: &mut Vec<u8>) {
        self.guid.serialize(buffer);
        self.guid.serialize(buffer);
        self.unknown2.serialize(buffer);
        self.unknown3.serialize(buffer);
        self.unknown4.serialize(buffer);
        self.unknown5.serialize(buffer);
        self.unknown6.serialize(buffer);
        self.unknown7.serialize(buffer);
        self.unknown8.serialize(buffer);
        self.base_cost.serialize(buffer);
        self.unknown10.serialize(buffer);
        self.unknown11.serialize(buffer);
        self.unknown12.serialize(buffer);
        self.member_cost.serialize(buffer);
    }
}

#[derive(SerializePacket)]
pub struct StoreItemList {
    pub static_items: Vec<StoreItem>,
    pub dynamic_items: Vec<StoreItem>,
}

impl GamePacket for StoreItemList {
    type Header = StoreOpCode;
    const HEADER: Self::Header = StoreOpCode::ItemList;
}

#[derive(SerializePacket)]
pub struct StoreItemDefinitionsReply {
    pub unknown: bool,
    pub defs: Vec<u32>,
}

impl GamePacket for StoreItemDefinitionsReply {
    type Header = StoreOpCode;
    const HEADER: Self::Header = StoreOpCode::ItemDefinitionsReply;
}
