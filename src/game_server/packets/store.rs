use num_enum::TryFromPrimitive;
use packet_serialize::{DeserializePacket, SerializePacket};

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
    /// Client → server: list of item GUIDs the client needs definitions for.
    /// Body: u32 count, then `count` × u32 item GUIDs.
    /// Server replies with `ItemDefinitionsReply` (0x3).
    ItemDefinitionsRequest = 0x2,
    ItemDefinitionsReply = 0x3,
    /// Client -> server: purchase request sent when the player clicks "Buy".
    /// Body: unknown u64, item_guid u32, currency_type u32, quantity u32.
    BuyItem = 0x4,
    /// Server -> client: `CoinStoreSellToClientResponsePacket`.
    /// Confirmed via Ghidra jump table at FUN_00b24dc0: sub-opcode 6 → case 2
    /// → calls FUN_00b24220, which on success calls the `MerchantPurchased`
    /// ExternalInterface function into Flash, closing CWAStoreWindowSelectedItem.
    ///
    /// Format (confirmed from log strings "Successfully transaction type: %d,
    /// tid: %d, item(s): %s, quantity: %d"):
    ///   result        u32   0 = CoinStoreTransactionResultSuccess
    ///   tid           u64   echo of BuyItemRequest.unknown (client transaction id)
    ///   transaction_type u32  0 = direct purchase
    ///   item_count    u32   number of items (always 1 for single-item groups)
    ///   item_guid     u32   (repeated item_count times)
    ///   quantity      u32
    SellToClientResponse = 0x6,
    /// Client -> server only; not part of the client's receive dispatch table.
    RequestItemList = 0x8,
}

/// Parsed body of a `BuyItem` (sub-opcode 4) packet.
#[derive(DeserializePacket)]
pub struct BuyItemRequest {
    pub unknown: u64,
    pub item_guid: u32,
    pub currency_type: u32,
    pub quantity: u32,
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

/// Server → client purchase confirmation (sub-opcode 6).
/// Confirmed via Ghidra: FUN_00b24220 parses this packet, then calls the Lua
/// functions `MerchantPurchased` (any purchase) and `MerchantPurchasedOne`
/// (single-item purchase) based on item_count.
///
/// Binary evidence from CloneWars.exe string table (offset 0x14562f8):
///   "Failed transaction type: %d, result: %d, tid: %d, item(s): %s, quantity: %d."
///   "Successfully transaction type: %d, tid: %d, item(s): %s, quantity: %d."
/// All numeric fields use %d (32-bit), confirming tid is u32, NOT u64.
/// (The same codebase uses %I64u for genuine 64-bit fields, e.g. merchant id.)
///
/// BuyItemRequest.unknown (u64) is the MERCHANT ID, not a client-generated tid.
/// The server generates its own tid (u32) for the response; we use 0.
pub struct SellToClientResponse {
    /// 1 = success.  Client handler 0xb24220 treats anything else as a failure
    /// ("Failed transaction ..." log, OnBuy reports failure).
    pub result: u32,
    /// Server-generated transaction id (u32).  Must be non-zero for the client to
    /// fire the MerchantPurchased / MerchantPurchasedOne Lua events.
    pub tid: u32,
    /// 1 = buy (client calls HandlerCoinStore.OnBuy), 2 = sell (OnSell); any
    /// other value fires no callback, which leaves the purchase window spinning.
    pub transaction_type: u32,
    /// Items purchased.
    pub item_guids: Vec<u32>,
    pub quantity: u32,
}

impl SerializePacket for SellToClientResponse {
    /// Layout read by the client (handler 0xb24220 → 0xb21150 → 0xb1dd70): after
    /// result/tid comes one transaction block describing ONE item:
    ///   transaction_type u32, u64, tid u32, u64,
    ///   item ref {item_def_id u32, item_guid u32}, quantity u32, u32, bool.
    /// HandlerCoinStore.OnBuy passes item_def_id to the purchase window
    /// (onCoinItemPurchase), which ticks items off its order with it — a wrong id
    /// leaves it on "Processing" until it times out.  quantity == 1 fires
    /// MerchantPurchasedOne, otherwise MerchantPurchased.  Multi-item purchases
    /// send one response per item (see `per_item`).
    fn serialize(&self, buffer: &mut Vec<u8>) {
        let item = self.item_guids.first().copied().unwrap_or(0);
        self.result.serialize(buffer);
        self.tid.serialize(buffer);
        self.transaction_type.serialize(buffer);
        0u64.serialize(buffer);
        self.tid.serialize(buffer);
        0u64.serialize(buffer);
        item.serialize(buffer); // item_def_id
        item.serialize(buffer); // item_guid (same as the definition id here)
        self.quantity.serialize(buffer);
        0u32.serialize(buffer);
        false.serialize(buffer);
    }
}

impl SellToClientResponse {
    /// One response per purchased item (the client's transaction block holds a
    /// single item reference).
    pub fn per_item(result: u32, transaction_type: u32, item_guids: &[u32], quantity: u32, mut next_tid: impl FnMut() -> u32) -> Vec<Self> {
        item_guids
            .iter()
            .map(|&guid| SellToClientResponse {
                result,
                tid: next_tid(),
                transaction_type,
                item_guids: vec![guid],
                quantity,
            })
            .collect()
    }
}

impl GamePacket for SellToClientResponse {
    type Header = StoreOpCode;
    const HEADER: Self::Header = StoreOpCode::SellToClientResponse;
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
