use enum_iterator::Sequence;
use num_enum::{IntoPrimitive, TryFromPrimitive};
use packet_serialize::{DeserializePacket, SerializePacket};
use serde::Deserialize;

use super::{player_update::CustomizationSlot, GamePacket, OpCode};

#[derive(Copy, Clone, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub enum ItemType {
    Equipment(EquipmentSlot),
    Customization(CustomizationSlot),
}

impl SerializePacket for ItemType {
    fn serialize(&self, buffer: &mut Vec<u8>) {
        match self {
            ItemType::Equipment(equipment_slot) => equipment_slot.serialize(buffer),
            ItemType::Customization(customization_slot) => customization_slot.serialize(buffer),
        }
    }
}

#[derive(
    Copy,
    Clone,
    Debug,
    Deserialize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    TryFromPrimitive,
    IntoPrimitive,
    SerializePacket,
    DeserializePacket,
    Sequence,
)]
#[serde(deny_unknown_fields)]
#[repr(u32)]
pub enum EquipmentSlot {
    None = 0,
    Head = 1,
    Hands = 2,
    Body = 3,
    Feet = 4,
    PrimaryWeapon = 7,
    SecondaryWeapon = 8,
    PrimarySaberShape = 10,
    PrimarySaberColor = 11,
    SecondarySaberShape = 12,
    SecondarySaberColor = 13,
}

impl EquipmentSlot {
    pub fn action_bar_priority(self) -> u32 {
        match self {
            EquipmentSlot::SecondaryWeapon => 1,
            EquipmentSlot::PrimaryWeapon => 2,
            EquipmentSlot::PrimarySaberShape => 3,
            EquipmentSlot::PrimarySaberColor => 3,
            EquipmentSlot::SecondarySaberShape => 4,
            EquipmentSlot::SecondarySaberColor => 4,
            EquipmentSlot::Head => 5,
            EquipmentSlot::Hands => 6,
            EquipmentSlot::Body => 7,
            EquipmentSlot::Feet => 8,
            EquipmentSlot::None => 0,
        }
    }

    pub fn is_weapon(self) -> bool {
        self == EquipmentSlot::PrimaryWeapon || self == EquipmentSlot::SecondaryWeapon
    }

    pub fn is_saber(self) -> bool {
        matches!(
            self,
            EquipmentSlot::PrimaryWeapon
                | EquipmentSlot::SecondaryWeapon
                | EquipmentSlot::PrimarySaberShape
                | EquipmentSlot::PrimarySaberColor
                | EquipmentSlot::SecondarySaberShape
                | EquipmentSlot::SecondarySaberColor
        )
    }

    pub fn opposite_slot(self) -> EquipmentSlot {
        match self {
            EquipmentSlot::PrimaryWeapon => EquipmentSlot::SecondaryWeapon,
            EquipmentSlot::SecondaryWeapon => EquipmentSlot::PrimaryWeapon,
            _ => EquipmentSlot::None,
        }
    }
}

impl From<ItemType> for EquipmentSlot {
    fn from(value: ItemType) -> Self {
        match value {
            ItemType::Equipment(equipment_slot) => equipment_slot,
            ItemType::Customization(_) => EquipmentSlot::None,
        }
    }
}

impl From<ItemType> for CustomizationSlot {
    fn from(value: ItemType) -> Self {
        match value {
            ItemType::Equipment(_) => CustomizationSlot::None,
            ItemType::Customization(customization_slot) => customization_slot,
        }
    }
}

#[derive(
    Copy,
    Clone,
    Debug,
    Deserialize,
    PartialEq,
    Eq,
    TryFromPrimitive,
    IntoPrimitive,
    SerializePacket,
    DeserializePacket,
)]
#[serde(deny_unknown_fields)]
#[repr(u32)]
pub enum WieldType {
    None = 0,
    SingleSaber = 1,
    StaffSaber = 2,
    ReverseSingleSaber = 3,
    DualSaber = 4,
    SinglePistol = 5,
    Rifle = 6,
    SniperRifle = 7,
    RocketLauncher = 8,
    FlameThrower = 9,
    DualPistol = 10,
    Staff = 11,
    Misc = 12,
    Bow = 13,
    Sparklers = 14,
    HeavyCannon = 15,
}

impl WieldType {
    pub fn holster(&self) -> WieldType {
        match *self {
            WieldType::SingleSaber
            | WieldType::DualSaber
            | WieldType::StaffSaber
            | WieldType::ReverseSingleSaber => WieldType::None,
            _ => *self,
        }
    }

    pub fn primary_slot(&self) -> EquipmentSlot {
        match self {
            WieldType::Bow => EquipmentSlot::SecondaryWeapon,
            _ => EquipmentSlot::PrimaryWeapon,
        }
    }

    /// Per-weapon-class Flourish move ids (AnimationTypes.xml type="12"),
    /// confirmed live in-game to self-terminate and carry baked-in
    /// effects/sound, unlike the looping Marketplace Preview poses (901-916)
    /// originally (and incorrectly) used for this. Each weapon class's own
    /// Locomotion-tree branch nests two move triads: FlourishPack1 (the 3
    /// individually-purchasable "Weapon Move 1/2/3" items, guids
    /// 2237/2238/2239) and FlourishPack2 (the "Taunting"/"Throwing" Weapon
    /// Move items, guids 2797/2798 - no purchasable item exists yet for a
    /// 3rd Pack2 move on any class, so the 3rd id in each pack2 array is
    /// unused for now but kept for parity with the client's animation data).
    ///
    /// Mapping confirmed via model_name cross-reference (NOT the WieldType
    /// enum's own naming, which is misleading - e.g. HeavyCannon, not
    /// FlameThrower, is the wield type actually used by every flamethrower
    /// weapon in weapons.yaml, and maps to the "HeavyHipGun" animation
    /// class). Returns None for wield types with no authored Flourish
    /// entries in the client and/or no items currently assigned to them
    /// (Misc, FlameThrower).
    pub fn flourish_packs(&self) -> Option<([i32; 3], [i32; 3])> {
        match self {
            WieldType::SingleSaber => Some(([110, 111, 112], [113, 114, 115])),
            WieldType::StaffSaber => Some(([130, 131, 132], [133, 134, 135])),
            WieldType::ReverseSingleSaber => Some(([150, 151, 152], [153, 154, 155])),
            WieldType::DualSaber => Some(([170, 171, 172], [173, 174, 175])),
            WieldType::Staff => Some(([190, 191, 192], [193, 194, 195])),
            WieldType::SinglePistol => Some(([210, 211, 212], [213, 214, 215])),
            WieldType::DualPistol => Some(([230, 231, 232], [233, 234, 235])),
            WieldType::Rifle => Some(([250, 251, 252], [253, 254, 255])),
            WieldType::SniperRifle => Some(([270, 271, 272], [273, 274, 275])),
            WieldType::HeavyCannon => Some(([290, 291, 292], [293, 294, 295])),
            WieldType::RocketLauncher => Some(([310, 311, 312], [313, 314, 315])),
            WieldType::Bow => Some(([350, 351, 352], [353, 354, 355])),
            WieldType::Sparklers => Some(([363, 364, 365], [366, 367, 368])),
            WieldType::None | WieldType::FlameThrower | WieldType::Misc => None,
        }
    }
}

#[derive(Clone, Deserialize, SerializePacket)]
pub struct Attachment {
    pub model_name: String,
    pub texture_alias: String,
    pub tint_alias: String,
    pub tint: u32,
    pub composite_effect: u32,
    pub slot: EquipmentSlot,
}

#[derive(SerializePacket, DeserializePacket)]
pub struct BaseAttachmentGroup {
    pub unknown1: u32,
    pub unknown2: String,
    pub unknown3: String,
    pub unknown4: u32,
    pub unknown5: String,
}

#[derive(Clone, SerializePacket)]
pub struct Item {
    pub definition_id: u32,
    pub tint: u32,
    pub guid: u32,
    pub quantity: u32,
    pub num_consumed: u32,
    pub last_use_time: u32,
    pub market_data: MarketData,
    pub unknown2: bool,
}

#[derive(Clone)]
pub enum MarketData {
    None,
    #[allow(dead_code)]
    Some(u64, u32, u32),
}

#[derive(Clone, Deserialize, SerializePacket)]
#[serde(deny_unknown_fields)]
pub struct ItemStat {}

#[derive(Clone, Deserialize, SerializePacket)]
#[serde(deny_unknown_fields)]
pub struct SpecialItemAbility {
    pub ability_slot: u32,
    pub ability_id: u32,
    pub unknown3: u32,
    pub ability_icon: u32,
    pub unknown5: u32,
    pub unknown6: u32,
    pub ability_name: u32,
}

#[derive(Clone, Deserialize, SerializePacket)]
#[serde(deny_unknown_fields)]
pub struct ItemDefinition {
    pub guid: u32,
    pub name_id: u32,
    pub description_id: u32,
    pub icon_set_id: u32,
    pub tint: u32,
    pub unknown6: u32,
    pub unknown7: u32,
    pub cost: u32,
    pub item_class: i32,
    pub required_battle_class: u32,
    pub slot: ItemType,
    pub disable_trade: bool,
    pub disable_sale: bool,
    pub model_name: String,
    pub texture_alias: String,
    pub required_gender: u32,
    pub item_type: u32,
    pub category: u32,
    pub members: bool,
    pub non_minigame: bool,
    pub weapon_trail_effect: u32,
    pub composite_effect: u32,
    pub power_rating: u32,
    pub min_battle_class_level: u32,
    pub rarity: u32,
    pub activatable_ability_id: u32,
    pub passive_ability_id: u32,
    pub single_use: bool,
    pub max_stack_size: i32,
    pub is_tintable: bool,
    pub tint_alias: String,
    pub disable_preview: bool,
    pub unknown33: bool,
    pub race_set_id: u32,
    pub unknown35: bool,
    pub unknown36: u32,
    pub unknown37: u32,
    pub customization_slot: CustomizationSlot,
    pub customization_id: u32,
    pub unknown40: u32,
    pub stats: Vec<ItemStat>,
    pub special_abilities: Vec<SpecialItemAbility>,
}

#[derive(SerializePacket, DeserializePacket)]
pub struct BrandishHolster {
    pub guid: u64,
}

impl GamePacket for BrandishHolster {
    type Header = OpCode;

    const HEADER: Self::Header = OpCode::BrandishHolster;
}
