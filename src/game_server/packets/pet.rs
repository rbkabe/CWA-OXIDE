use packet_serialize::SerializePacket;

use super::{player_data::Pet, GamePacket, OpCode};

// ────────────────────────────────────────────────────────────────────────────
// Wire format (confirmed against CloneWars.exe)
// ────────────────────────────────────────────────────────────────────────────
//
// Server→client pet packets use the top-level opcode OpCode::Pet (0x35) — the
// same opcode the client uses for its own pet packets (e.g. summon).  The
// client's game-packet router (0x7c2930) switches on that opcode; 0x35 goes to
// player->0x81ad50 → pet controller (player+0x860) → 0x8c14e0.  Any other
// opcode (OpCode::Companion = 0x19 in particular) is silently dropped.
//
// 0x8c14e0 reads the header with 0x8ba160: [i16 opcode][i8 sub-op], then
// dispatches on the sub-op (lookup table 0x8c36f4, jump table 0x8c36a0):
//   0x01 → 0x8c1584  (PetActive: 3×u32, meaning unconfirmed — not sent)
//   0x03 → 0x8c1e63  PetStatusUpdated
//   0x05 → 0x8c1f67  PetInventory (replaces the client's pet-record list)
//   0x0b → 0x8c3269  refresh of BaseClient.PetInventory.EquippedWearablesData
//
// GamePacket serialization writes the opcode; each body below starts with the
// one-byte sub-op.
// ────────────────────────────────────────────────────────────────────────────

/// Sub-op 0x05: replaces the client's list of pet records.
///
/// The handler (0x8c1f67) clears the pet controller's record list and refills
/// it from `pets`, keyed by each record's `pet_id`.  That list backs
/// `BaseClient.PetList` (HUD / pet list window) and is what the
/// `BaseClient.PetInventory` (attachment flyout) filter looks the active pet up
/// in, so it must contain EVERY pet the player owns, not just the active one.
///
/// ```text
/// [0x05]                        sub-op
/// [count u32]
/// count × ([pet_id u32] [Pet])  record key, then the record (parser 0x78af80)
/// [active_pet_id u32]           → pet_controller[0x84]: the client's ACTIVE pet id
/// [companion_guid lo u32]       → pet_controller[0x88]
/// [companion_guid hi u32]       → pet_controller[0x8c]
/// ```
///
/// When the trailing values differ from the stored ones, the client stores them
/// (0x8bdd00) and looks up the companion NPC by `companion_guid`.  The
/// attachment flyout only lists an attachment if a record keyed
/// `active_pet_id` exists and the attachment's `item_class` is a key in that
/// record's `battle_class_items` map.
pub struct PetInventory {
    pub pets: Vec<Pet>,
    pub active_pet_id: u32,
    /// GUID of the active companion's NPC (as sent in its AddNpc).
    pub companion_guid: u64,
}

impl SerializePacket for PetInventory {
    fn serialize(&self, buffer: &mut Vec<u8>) {
        0x05u8.serialize(buffer);
        (self.pets.len() as u32).serialize(buffer);
        for pet in &self.pets {
            pet.pet_id.serialize(buffer);
            pet.serialize(buffer);
        }
        self.active_pet_id.serialize(buffer);
        ((self.companion_guid & 0xFFFF_FFFF) as u32).serialize(buffer);
        ((self.companion_guid >> 32) as u32).serialize(buffer);
    }
}

impl GamePacket for PetInventory {
    type Header = OpCode;
    const HEADER: Self::Header = OpCode::Pet;
}

// ────────────────────────────────────────────────────────────────────────────

/// Sub-op 0x01.  NOT CURRENTLY SENT.
///
/// The handler (0x8c1584) reads three u32s (generic reader 0x8ba1c0), stores a
/// record keyed by the FIRST value and fires a speaker/listener event.  The
/// field names below are an old guess; sending it with the attachment group as
/// the first value grayed out the ActivePetWindow gear icon, so its real
/// meaning needs to be worked out before using it.
pub struct PetActive {
    pub attachment_item_group_id: u32,
    pub is_vanity_pet: bool,
    pub image_set_id: u32,
}

impl SerializePacket for PetActive {
    fn serialize(&self, buffer: &mut Vec<u8>) {
        0x01u8.serialize(buffer);
        self.attachment_item_group_id.serialize(buffer);
        (self.is_vanity_pet as u32).serialize(buffer);
        self.image_set_id.serialize(buffer);
    }
}

impl GamePacket for PetActive {
    type Header = OpCode;
    const HEADER: Self::Header = OpCode::Pet;
}

// ────────────────────────────────────────────────────────────────────────────

/// Sub-op 0x03 (handler 0x8c1e63): updates one pet record.  Not yet verified
/// in game.
pub struct PetStatusUpdate {
    pub pet: Pet,
}

impl SerializePacket for PetStatusUpdate {
    fn serialize(&self, buffer: &mut Vec<u8>) {
        0x03u8.serialize(buffer);
        self.pet.serialize(buffer);
    }
}

impl GamePacket for PetStatusUpdate {
    type Header = OpCode;
    const HEADER: Self::Header = OpCode::Pet;
}

// ────────────────────────────────────────────────────────────────────────────

/// Sub-op 0x10: stores `pet.utilities[slot] = item_id` and fires Lua
/// `"PetsHandler:SetPetUtility"(slot, item_id)`.  Not yet verified in game.
pub struct PetSetUtility {
    pub slot: u32,
    pub item_id: u32,
}

impl SerializePacket for PetSetUtility {
    fn serialize(&self, buffer: &mut Vec<u8>) {
        0x10u8.serialize(buffer);
        self.slot.serialize(buffer);
        self.item_id.serialize(buffer);
    }
}

impl GamePacket for PetSetUtility {
    type Header = OpCode;
    const HEADER: Self::Header = OpCode::Pet;
}

// ────────────────────────────────────────────────────────────────────────────

/// Sub-op 0x0b: equip-item message.  Same layout in both directions
/// (parser 0x8baa30): `[flag u8][slot u32][item_id u32]`.  The client sends
/// `00 | 01 00 00 00 | <item>` when an attachment is clicked
/// (Lua `Pet.EquipItem(item, 1)`).  The client's handler for the server copy
/// (0x8c3269) refreshes `BaseClient.PetInventory.EquippedWearablesData`.
pub struct PetEquipAttachment {
    pub flag: bool,
    pub slot: u32,
    pub item_id: u32,
}

impl SerializePacket for PetEquipAttachment {
    fn serialize(&self, buffer: &mut Vec<u8>) {
        0x0bu8.serialize(buffer);
        (self.flag as u8).serialize(buffer);
        self.slot.serialize(buffer);
        self.item_id.serialize(buffer);
    }
}

impl GamePacket for PetEquipAttachment {
    type Header = OpCode;
    const HEADER: Self::Header = OpCode::Pet;
}
