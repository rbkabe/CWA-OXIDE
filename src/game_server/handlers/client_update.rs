use num_enum::{IntoPrimitive, TryFromPrimitive};
use packet_serialize::{DeserializePacket, SerializePacket};

use crate::game_server::packets::{ActionBarSlot, ActionBarType};

use super::{
    item::{Attachment, EquipmentSlot, Item, ItemDefinition},
    GamePacket, OpCode, Pos,
};

#[derive(Copy, Clone, Debug)]
pub enum ClientUpdateOpCode {
    Health = 0x1,
    AddItems = 0x2,
    EquipItem = 0x5,
    UnequipItem = 0x6,
    Stats = 0x7,
    CollectionStart = 0x8,
    CollectionRemove = 0x9,
    CollectionAddEntry = 0xa,
    CollectionRemoveEntry = 0xb,
    Position = 0xc,
    Power = 0xd,
    UpdateCredits = 0x13,
    UpdateActionBarSlot = 0x19,
    PreloadCharactersDone = 0x1a,
}

impl SerializePacket for ClientUpdateOpCode {
    fn serialize(&self, buffer: &mut Vec<u8>) {
        OpCode::ClientUpdate.serialize(buffer);
        (*self as u16).serialize(buffer);
    }
}

#[derive(SerializePacket, DeserializePacket)]
pub struct Position {
    pub player_pos: Pos,
    pub rot: Pos,
    pub is_teleport: bool,
    pub unknown2: bool,
}

impl GamePacket for Position {
    type Header = ClientUpdateOpCode;
    const HEADER: Self::Header = ClientUpdateOpCode::Position;
}

#[derive(SerializePacket)]
pub struct AddItemsData {
    pub item: Item,
    pub definition: ItemDefinition,
}

pub struct AddItems {
    pub data: AddItemsData,
}

impl SerializePacket for AddItems {
    fn serialize(&self, buffer: &mut Vec<u8>) {
        let mut inner_buffer = Vec::new();
        self.data.serialize(&mut inner_buffer);
        inner_buffer.serialize(buffer);
    }
}

impl GamePacket for AddItems {
    type Header = ClientUpdateOpCode;
    const HEADER: Self::Header = ClientUpdateOpCode::AddItems;
}

#[derive(SerializePacket)]
pub struct EquipItem {
    pub item_guid: u32,
    pub attachment: Attachment,
    pub battle_class: u32,
    pub item_class: i32,
    pub equip: bool,
}

impl GamePacket for EquipItem {
    type Header = ClientUpdateOpCode;
    const HEADER: Self::Header = ClientUpdateOpCode::EquipItem;
}

#[derive(SerializePacket)]
pub struct UnequipItem {
    pub slot: EquipmentSlot,
    pub battle_class: u32,
}

impl GamePacket for UnequipItem {
    type Header = ClientUpdateOpCode;
    const HEADER: Self::Header = ClientUpdateOpCode::UnequipItem;
}

#[derive(SerializePacket, DeserializePacket)]
pub struct Health {
    pub current: u32,
    pub max: u32,
}

impl GamePacket for Health {
    type Header = ClientUpdateOpCode;
    const HEADER: ClientUpdateOpCode = ClientUpdateOpCode::Health;
}

#[derive(SerializePacket, DeserializePacket)]
pub struct Power {
    pub current: u32,
    pub max: u32,
}

impl GamePacket for Power {
    type Header = ClientUpdateOpCode;
    const HEADER: ClientUpdateOpCode = ClientUpdateOpCode::Power;
}

#[derive(
    Copy, Clone, Debug, TryFromPrimitive, IntoPrimitive, SerializePacket, DeserializePacket,
)]
#[repr(u32)]
pub enum StatId {
    MaxHealth = 1,
    Speed = 2,
    Range = 3,
    HealthRegen = 4,
    MaxPower = 5,
    PowerRegen = 6,
    MeleeDefense = 7,
    MeleeDodge = 8,
    MeleeCritRate = 9,
    MeleeCritMultiplier = 10,
    MeleeAccuracy = 11,
    WeaponDamageMultiplier = 12,
    HandToHandDamage = 13,
    WeaponDamage = 14,
    WeaponSpeed = 15,
    DamageReductionFlat = 16,
    ExperienceBoost = 17,
    DamageReductionPct = 18,
    DamageAddition = 19,
    DamageMultiplier = 20,
    HealingAddition = 21,
    HealingMultiplier = 22,
    AbilityCritRate = 33,
    AbilityCritMultiplier = 34,
    Luck = 35,
    HeadInflation = 36,
    CurrencyBoost = 37,
    Toughness = 50,
    AbilityCritVulnerability = 51,
    MeleeCritVulnerability = 52,
    RangeMultiplier = 53,
    MaxShield = 54,
    ShieldRegen = 55,
    MimicMovementSpeed = 57,
    GravityMultiplier = 58,
    JumpHeightMultiplier = 59,
}

#[derive(SerializePacket)]
pub struct Stat {
    pub id: StatId,
    pub multiplier: u32,
    pub value1: f32,
    pub value2: f32,
}

#[derive(SerializePacket)]
pub struct Stats {
    pub stats: Vec<Stat>,
}

impl GamePacket for Stats {
    type Header = ClientUpdateOpCode;
    const HEADER: ClientUpdateOpCode = ClientUpdateOpCode::Stats;
}

#[derive(SerializePacket, DeserializePacket)]
pub struct UpdateCredits {
    pub new_credits: u32,
}

impl GamePacket for UpdateCredits {
    type Header = ClientUpdateOpCode;

    const HEADER: Self::Header = ClientUpdateOpCode::UpdateCredits;
}

#[derive(SerializePacket, DeserializePacket)]
pub struct UpdateActionBarSlot {
    pub action_bar_type: ActionBarType,
    pub slot_index: u32,
    pub slot: ActionBarSlot,
}

impl GamePacket for UpdateActionBarSlot {
    type Header = ClientUpdateOpCode;
    const HEADER: ClientUpdateOpCode = ClientUpdateOpCode::UpdateActionBarSlot;
}

#[derive(SerializePacket, DeserializePacket)]
pub struct PreloadCharactersDone {
    pub unknown1: bool,
}

impl GamePacket for PreloadCharactersDone {
    type Header = ClientUpdateOpCode;
    const HEADER: ClientUpdateOpCode = ClientUpdateOpCode::PreloadCharactersDone;
}

/// Sent when a player collects one piece of a collection set.
///
/// Wire format (after the 0x26/0x0a opcode pair):
///   u16  collection_id   — identifies the collection set (cast of set name_id)
///   u16  slot            — 0-based index of this piece within the set
///   u32  item_name_id    — string ID of the collected piece (object+0x0c)
///   u32  log_field1      — Loggable sub-obj field (object+0x14)
///   u32  log_field2      — Loggable sub-obj field (object+0x18)
///   u32  unknown4        — object+0x1c
///   u32  unknown5        — object+0x20
///   u32  unknown6        — object+0x24
///   u32  unknown7        — object+0x28
///   bool is_complete     — true when this piece completes the set (object+0x2c)
#[derive(SerializePacket)]
pub struct CollectionAddEntry {
    pub collection_id: u16,
    pub slot: u16,
    pub item_name_id: u32,
    pub log_field1: u32,
    pub log_field2: u32,
    pub unknown4: u32,
    pub unknown5: u32,
    pub unknown6: u32,
    pub unknown7: u32,
    pub is_complete: bool,
}

impl GamePacket for CollectionAddEntry {
    type Header = ClientUpdateOpCode;
    const HEADER: ClientUpdateOpCode = ClientUpdateOpCode::CollectionAddEntry;
}

/// Sent on login for each collection set that the player has started.
///
/// Wire format (after the 0x26/0x08 opcode pair):
///   u16  collection_id   — identifies the collection set (used as DS row key AND locale
///                          string ID; the SWF calls GetStringById(id) for the display name)
///   u16  unknown1        — category_id (2-5 per CollectionCategories.txt); used by
///                          Ui.SetCollectionFilterByCategory to group collections by zone
///   i32  blob_len        — byte length of the following blob (should be 16)
///   blob (4 × u32 LE)   — C++ collection object fields (EXE fn 0x0037d2a0):
///                          [0] → [collection+0x9c] = collection_id (DS row key)
///                          [1] → [collection+0xa0] = category_id (MUST match zone
///                                 filter; Umbara=2, or collections are invisible)
///                          [2] → [collection+0xa4] = image_set_id (DS.imageid)
///                          [3] → [collection+0xa8] = entry_count  (DS.entryCount)
pub struct CollectionStart {
    pub collection_id: u16,
    pub unknown1: u16,
    /// Raw blob — may be empty (len=0) until blob format is fully understood.
    pub blob: Vec<u8>,
}

impl SerializePacket for CollectionStart {
    fn serialize(&self, buffer: &mut Vec<u8>) {
        self.collection_id.serialize(buffer);
        self.unknown1.serialize(buffer);
        (self.blob.len() as i32).serialize(buffer);
        buffer.extend_from_slice(&self.blob);
    }
}

impl GamePacket for CollectionStart {
    type Header = ClientUpdateOpCode;
    const HEADER: ClientUpdateOpCode = ClientUpdateOpCode::CollectionStart;
}
