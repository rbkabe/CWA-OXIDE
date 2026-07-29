use std::collections::BTreeMap;

use packet_serialize::LengthlessVec;

use crate::game_server::{
    handlers::{character::PlayerAbilityGroup, item::ItemConfig},
    packets::{
        client_update::{CollectionAddEntry, CollectionStart},
        item::{EquipmentSlot, Item, MarketData},
        player_data::{
            AbilityType, ActionBar, BattleClass, BattleClassItem, BattleClassUnknown10,
            EquippedItem, InventoryItem, Item2, ItemGuid, Mount, Pet, PetTrick, Player, PlayerData,
            Unknown12, Unknown13, Unknown2,
        },
        player_update::{CustomizationSlot, NameplateImage, NameplateImageId},
        tunnel::TunneledPacket,
        AbilitySubType, ActionBarSlot, ActionBarType, GamePacket, Name, Pos,
    },
};

use super::{
    guid::Guid,
    mount::MountConfig,
    unique_guid::{mount_guid, player_guid},
};

fn make_pet(guid: u32, name: &str, icon_id: u32, attachment_group: u32, attachments: &[u32]) -> Pet {
    // item_guid[0] = the companion's own definition guid.
    // item_guid[1..] = owned attachment item definition guids; the client reads these
    // to populate BaseClient.PetInventory (the companion gear/attachment flyout).
    let mut item_guid = vec![ItemGuid { guid }];
    for &att in attachments {
        item_guid.push(ItemGuid { guid: att });
    }
    Pet {
        pet_id: guid,
        unknown2: false,
        unknown3: 0,
        food: 0.0,
        groom: 0.0,
        exercise: 0.0,
        happiness: 0.0,
        unknown8: false,
        pet_trick: vec![PetTrick {
            unknown1: 0,
            unknown2: Unknown2 {
                unknown1: 0,
                unknown2: 0,
                unknown3: 0,
                unknown4: 0,
                unknown5: 0,
                unknown6: 0,
                unknown7: 0,
                unknown8: 0,
                unknown9: false,
            },
        }],
        item_guid,
        battle_class_items: vec![BattleClassItem {
            item1: 0,
            item2: Item2 { unknown1: 0, unknown2: 0 },
        }],
        pet_name: name.to_string(),
        tint_id: 0,
        texture_alias: "".to_string(),
        icon_id,
        unknown10: false,
        unknown11: 0,
        // unknown12.unknown1 = "Attachment ItemGroup ID" in BaseClient.PetList.
        // Lua's PetController::SetActivePetId reads this column and calls
        // setPetAttachmentItemGroupId(value); value=0 causes C++ to mark the pet
        // as "no attachments" and discard PetInventory DataSource updates.
        unknown12: Unknown12 { unknown1: attachment_group, unknown2: 0, unknown3: 0, unknown4: 0 },
        unknown13: Unknown13 {
            unknown1: 0, unknown2: 0, unknown3: 0, unknown4: 0,
            unknown5: 0, unknown6: 0, unknown7: 0, unknown8: 0,
        },
    }
}

pub fn make_test_player(
    guid: u32,
    mounts: &BTreeMap<u32, MountConfig>,
    items: &BTreeMap<u32, ItemConfig>,
) -> Player {
    let mut owned_mounts = Vec::new();
    for mount in mounts.values() {
        owned_mounts.push(Mount {
            mount_id: mount.guid(),
            name_id: mount.name_id,
            icon_set_id: mount.icon_set_id,
            guid: mount_guid(player_guid(guid)),
            unknown5: false,
            unknown6: 0,
            unknown7: "".to_string(),
        })
    }

    let mut inventory = BTreeMap::new();
    for item in items.values() {
        inventory.insert(
            item.guid,
            InventoryItem {
                definition_id: item.guid,
                item: Item {
                    definition_id: item.guid,
                    tint: item.tint,
                    guid: item.guid,
                    quantity: 1,
                    num_consumed: 0,
                    last_use_time: 0,
                    market_data: MarketData::None,
                    unknown2: false,
                },
            },
        );
    }

    Player {
        data: PlayerData {
            account_guid: 0,
            player_guid: player_guid(guid),
            body_model: 484,
            head_model: String::from("Char_CloneHead.adr"),
            hair_model: String::from("Cust_Clone_Hair_BusinessMan.adr"),
            hair_color: 11,
            eye_color: 0,
            skin_tone: String::from("CloneTan"),
            face_paint: String::from("SquarishTattoo"),
            facial_hair: String::from(""),
            head_customization_id: 0,
            hair_style_customization_id: 0,
            skin_tone_customization_id: 0,
            face_design_customization_id: 0,
            model_customization_id: 0,
            pos: Pos {
                x: 887.3,
                y: 171.93376,
                z: 1546.956,
                w: 1.0,
            },
            rot: Pos {
                x: 1.5,
                y: 0.0,
                z: 0.0,
                w: 0.0,
            },
            name: Name {
                first_name_id: 0,
                middle_name_id: 0,
                last_name_id: 0,
                first_name: String::from("rbkabe"),
                last_name: if guid == 1 {
                    String::from(" ")
                } else {
                    format!("  {guid}")
                },
            },
            credits: 1000000,
            account_creation_date: 1261854072,
            account_age: 0,
            account_play_time: 0,
            membership_unknown1: true,
            membership_unknown2: true,
            membership_unknown3: true,
            membership_unknown4: true,
            unknown9: 217,
            unknown10: 2,
            unknown11: 0,
            unknown12: 500,
            unknown13: 1,
            unknown14: false,
            unknown15: 3,
            unknown16: 5,
            equipped_vehicles: vec![],
            battle_classes: BTreeMap::from([(
                1,
                BattleClass {
                    guid: 1,
                    name_id: 52577,
                    description_id: 2837,
                    selected_ability: 0,
                    icon_id: 6442,
                    unknown1: 0,
                    badge_background_id: 0,
                    badge_id: 0,
                    members_only: false,
                    is_combat: 1,
                    item_class_data: vec![],
                    unknown2: false,
                    unknown3: 0,
                    unknown4: 1931819892,
                    unknown5: false,
                    unknown6: 0,
                    unknown7: vec![],
                    level: 1,
                    xp_in_level: 0,
                    total_xp: 0,
                    unknown8: 0,
                    items: BTreeMap::from([
                        (
                            EquipmentSlot::Head,
                            EquippedItem {
                                slot: EquipmentSlot::Head,
                                guid: 3276,
                                category: 0,
                            },
                        ),
                        (
                            EquipmentSlot::Hands,
                            EquippedItem {
                                slot: EquipmentSlot::Hands,
                                guid: 3277,
                                category: 0,
                            },
                        ),
                        (
                            EquipmentSlot::Body,
                            EquippedItem {
                                slot: EquipmentSlot::Body,
                                guid: 2699,
                                category: 0,
                            },
                        ),
                        (
                            EquipmentSlot::Feet,
                            EquippedItem {
                                slot: EquipmentSlot::Feet,
                                guid: 3199,
                                category: 0,
                            },
                        ),
                        (
                            EquipmentSlot::PrimaryWeapon,
                            EquippedItem {
                                slot: EquipmentSlot::PrimaryWeapon,
                                guid: 437,
                                category: 0,
                            },
                        ),
			(
			    EquipmentSlot::PrimarySaberShape,
			    EquippedItem {
				slot: EquipmentSlot::PrimarySaberShape,
				guid: 90009,
				category: 0,
			    },
			),
			(
			    EquipmentSlot::PrimarySaberColor,
			    EquippedItem {
				slot: EquipmentSlot::PrimarySaberColor,
				guid: 100011,
				category: 0,
			    },
			),
                    ]),
                    unknown9: 0,
                    abilities: vec![
                        AbilityType::Empty,
                        AbilityType::Empty,
                        AbilityType::Empty,
                        AbilityType::Empty,
                        AbilityType::Empty,
                        AbilityType::Empty,
                        AbilityType::Empty,
                        AbilityType::Empty,
                    ],
                    unknown10: LengthlessVec(vec![BattleClassUnknown10::None]),
                },
            )]),
            active_battle_class: 1,
            unknown: vec![],
            social: vec![],
            inventory,
            gender: 1,
            quests: vec![],
            quests_unknown1: 241,
            quests_unknown2: 2513,
            quests_unknown3: true,
            quests_unknown4: 10,
            quests_unknown5: 30,
            achievements: vec![],
            acquaintances: vec![],
            recipes: vec![],
            pets: vec![
                // Protocol droids — group 175 parts: 311/430/447
                make_pet(541,  "C-3PO",   74,   175, &[311, 430, 447]),
                make_pet(550,  "N0-80T",  77,   175, &[311, 430, 447]),
                make_pet(551,  "N-30H",   78,   175, &[311, 430, 447]),
                make_pet(552,  "J3-3V3",  76,   175, &[311, 430, 447]),
                make_pet(553,  "D-0T",    75,   175, &[311, 430, 447]),
                make_pet(998,  "RA-7",    1231, 175, &[311, 430, 447]),
                // Mouse droids — group 168 parts: 424/444
                make_pet(542,  "F1-V3L",  71,   168, &[424, 444]),
                make_pet(2092, "1M-AU5",  1258, 168, &[424, 444]),
                // Probe droid — group 171 parts: 657/658/659
                make_pet(543,  "PR-0B07", 73,   171, &[657, 658, 659]),
                // Power droid — group 173 parts: 428/429/436
                make_pet(544,  "5T-U85",  72,   173, &[428, 429, 436]),
                // Astromech droids — group 174 parts: 309/310/427
                make_pet(545,  "R2-D2",   64,   174, &[309, 310, 427]),
                make_pet(546,  "R3-S6",   66,   174, &[309, 310, 427]),
                make_pet(547,  "B3-3P5",  67,   174, &[309, 310, 427]),
                make_pet(548,  "M1-L0",   68,   174, &[309, 310, 427]),
                make_pet(549,  "B0-LT5",  69,   174, &[309, 310, 427]),
                make_pet(916,  "R2-KT",   1075, 174, &[309, 310, 427]),
                make_pet(917,  "B3-T4",   1035, 174, &[309, 310, 427]),
                make_pet(2146, "6R-0WL",  1464, 174, &[309, 310, 427]),
                make_pet(2147, "P3-NUT",  1465, 174, &[309, 310, 427]),
                make_pet(2393, "H3-4RT",  1675, 174, &[309, 310, 427]),
                make_pet(2804, "7L-VN",   1998, 174, &[309, 310, 427]),
                // AT-AT — group 169 parts: 563/564
                make_pet(632,  "5P-0T",   63,   169, &[563, 564]),
                // Techno-Service droid — group 172 parts: 560/561/562
                make_pet(633,  "T0-D0",   79,   172, &[560, 561, 562]),
                // Rabbit droid — no confirmed attachment group
                make_pet(554,  "BR-3R",   70,   0, &[]),
                // Unknown/unconfirmed droid types — no confirmed attachment group
                make_pet(2114, "J-4CK",   1385, 0, &[]),
                make_pet(2115, "K-3RNL",  1384, 0, &[]),
                make_pet(3080, "DU-3",    2392, 0, &[]),
                make_pet(3081, "K0-5D",   2393, 0, &[]),
                make_pet(3284, "BR-RR",   2574, 0, &[]),
                // Creatures
                make_pet(2752, "Aedalus",  1846, 641, &[2715, 2716]),
                make_pet(2786, "Gnarls",   1976, 0, &[]),
                make_pet(2802, "Eeetch",   1997, 0, &[]),
                make_pet(2899, "Vector",   2143, 0, &[]),
                make_pet(3073, "Babbo",    2357, 0, &[]),
                make_pet(3074, "Duppa",    2358, 0, &[]),
                make_pet(3100, "Fang",     2587, 0, &[]),
                make_pet(3219, "Erial",    2463, 0, &[]),
                make_pet(3282, "Phileas",  2572, 837, &[3274, 3275]),
                make_pet(3283, "Barnibus", 2573, 843, &[3285]),
                make_pet(3366, "Marrok",   2667, 0, &[]),
                make_pet(3373, "Ghorroh",  2700, 0, &[]),
                // Missing companions (GUIDs assigned by emulator)
                make_pet(3420, "Skreech",                2800, 0, &[]),
                make_pet(3421, "Cinch",                  2801, 0, &[]),
                make_pet(3422, "Amelie",                 2802, 0, &[]),
                make_pet(3423, "Gulsh",                  2803, 0, &[]),
                make_pet(3424, "Kharroh",                2804, 0, &[]),
                make_pet(3415, "Gash",                   2805, 0, &[]),
                make_pet(3406, "Cercopes",               2806, 0, &[]),
                make_pet(3407, "WAC-47",                 2807, 0, &[]),
                make_pet(3408, "M5-BZ",                  2808, 174, &[309, 310, 427]),
                make_pet(3425, "U9-C4",                  2809, 174, &[309, 310, 427]),
                make_pet(3440, "BNI-393",                2810, 0, &[]),
                make_pet(3446, "Security Battle Droid",  2811, 0, &[]),
                make_pet(3447, "L1-LR3D",                2812, 0, &[]),
                // Sidekicks — no attachment gear
                make_pet(3079, "Aktik", 1961, 0, &[]),
                make_pet(3341, "Jynx",  1341, 0, &[]),
                make_pet(3398, "Klug",  2131, 0, &[]),
            ],
            pet_unknown1: -1,
            pet_unknown2: 0,
            mounts: owned_mounts,
            action_bars: vec![
                ActionBar {
                    action_bar_type: ActionBarType::Weapon,
                    slots: vec![
                        ActionBarSlot {
                            is_empty: false,
                            icon_id: 219,
                            icon_tint_id: 0,
                            name_id: 60026,
                            ability_type: 0,
                            ability_sub_type: AbilitySubType::InstantSingleTarget,
                            area_of_effect_radius: 0.0,
                            max_distance_from_player: 0.0,
                            required_force_points: 0,
                            is_enabled: true,
                            use_cooldown_millis: 0,
                            init_cooldown_millis: 0,
                            unknown13: 0,
                            quantity: 0,
                            is_consumable: false,
                            millis_since_last_use: 0,
                        },
                        ActionBarSlot {
                            is_empty: false,
                            icon_id: 3179,
                            icon_tint_id: 0,
                            name_id: 7,
                            ability_type: 0,
                            ability_sub_type: AbilitySubType::InstantSingleTarget,
                            area_of_effect_radius: 0.0,
                            max_distance_from_player: 0.0,
                            required_force_points: 0,
                            is_enabled: true,
                            use_cooldown_millis: 0,
                            init_cooldown_millis: 0,
                            unknown13: 0,
                            quantity: 0,
                            is_consumable: false,
                            millis_since_last_use: 0,
                        },
                        ActionBarSlot {
                            is_empty: false,
                            icon_id: 3754,
                            icon_tint_id: 0,
                            name_id: 60323,
                            ability_type: 0,
                            ability_sub_type: AbilitySubType::InstantSingleTarget,
                            area_of_effect_radius: 0.0,
                            max_distance_from_player: 0.0,
                            required_force_points: 0,
                            is_enabled: true,
                            use_cooldown_millis: 0,
                            init_cooldown_millis: 0,
                            unknown13: 0,
                            quantity: 0,
                            is_consumable: false,
                            millis_since_last_use: 0,
                        },
                        ActionBarSlot {
                            is_empty: true,
                            icon_id: 0,
                            icon_tint_id: 0,
                            name_id: 0,
                            ability_type: 0,
                            ability_sub_type: AbilitySubType::InstantSingleTarget,
                            area_of_effect_radius: 0.0,
                            max_distance_from_player: 0.0,
                            required_force_points: 0,
                            is_enabled: false,
                            use_cooldown_millis: 0,
                            init_cooldown_millis: 0,
                            unknown13: 0,
                            quantity: 0,
                            is_consumable: false,
                            millis_since_last_use: 0,
                        },
                    ],
                },
                ActionBar {
                    action_bar_type: ActionBarType::Consumable,
                    slots: vec![
                        ActionBarSlot {
                            is_empty: true,
                            icon_id: 0,
                            icon_tint_id: 0,
                            name_id: 0,
                            ability_type: 0,
                            ability_sub_type: AbilitySubType::CastableSingleTarget,
                            area_of_effect_radius: 0.0,
                            max_distance_from_player: 0.0,
                            required_force_points: 0,
                            is_enabled: true,
                            use_cooldown_millis: 0,
                            init_cooldown_millis: 0,
                            unknown13: 0,
                            quantity: 0,
                            is_consumable: true,
                            millis_since_last_use: 0,
                        },
                        ActionBarSlot {
                            is_empty: true,
                            icon_id: 0,
                            icon_tint_id: 0,
                            name_id: 0,
                            ability_type: 0,
                            ability_sub_type: AbilitySubType::CastableSingleTarget,
                            area_of_effect_radius: 0.0,
                            max_distance_from_player: 0.0,
                            required_force_points: 0,
                            is_enabled: true,
                            use_cooldown_millis: 0,
                            init_cooldown_millis: 0,
                            unknown13: 0,
                            quantity: 0,
                            is_consumable: true,
                            millis_since_last_use: 0,
                        },
                        ActionBarSlot {
                            is_empty: true,
                            icon_id: 0,
                            icon_tint_id: 0,
                            name_id: 0,
                            ability_type: 0,
                            ability_sub_type: AbilitySubType::CastableSingleTarget,
                            area_of_effect_radius: 0.0,
                            max_distance_from_player: 0.0,
                            required_force_points: 0,
                            is_enabled: true,
                            use_cooldown_millis: 0,
                            init_cooldown_millis: 0,
                            unknown13: 0,
                            quantity: 0,
                            is_consumable: true,
                            millis_since_last_use: 0,
                        },
                        ActionBarSlot {
                            is_empty: true,
                            icon_id: 0,
                            icon_tint_id: 0,
                            name_id: 0,
                            ability_type: 0,
                            ability_sub_type: AbilitySubType::CastableSingleTarget,
                            area_of_effect_radius: 0.0,
                            max_distance_from_player: 0.0,
                            required_force_points: 0,
                            is_enabled: true,
                            use_cooldown_millis: 0,
                            init_cooldown_millis: 0,
                            unknown13: 0,
                            quantity: 0,
                            is_consumable: true,
                            millis_since_last_use: 0,
                        },
                    ],
                },
            ],
            unknown17: false,
            matchmaking_queues: vec![],
            minigame_tutorials: vec![],
            power_hours: vec![],
            stats: vec![],
            vehicle_unknown1: 0,
            vehicles: vec![],
            titles: vec![],
            equipped_title: 0,
            unknown18: vec![],
            effects: vec![],
        },
    }
}

pub fn make_test_weapon_abilities() -> Vec<PlayerAbilityGroup> {
    vec![PlayerAbilityGroup {
        source_item_id: 2909,
        ability_keys: vec![
            "vigilance".to_string(),
            "thermal_grenade".to_string(),
            "focused_shot".to_string(),
        ],
        priority: 2,
    }]
}

pub fn make_test_customizations() -> BTreeMap<CustomizationSlot, u32> {
    let mut customizations = BTreeMap::new();
    customizations.insert(CustomizationSlot::HeadModel, 110000);
    customizations.insert(CustomizationSlot::SkinTone, 60030);
    customizations.insert(CustomizationSlot::HairStyle, 40034);
    customizations.insert(CustomizationSlot::HairColor, 30004);
    customizations.insert(CustomizationSlot::EyeColor, 10013);
    customizations.insert(CustomizationSlot::FacialHair, 20004);
    customizations.insert(CustomizationSlot::FacePattern, 50009);
    customizations.insert(CustomizationSlot::BodyModel, 70000);
    customizations
}

/// Builds hardcoded collection packets for diagnostic testing.
/// Sends one complete collection (all 8 pieces) for each of the 4 valid
/// category IDs (2-5), so if ANY category or format variant works we'll see it.
/// Remove this function once the real collection flow is confirmed working.
pub fn make_test_collection_replay() -> Vec<Vec<u8>> {
    let mut pkts = Vec::new();

    // Try all 4 valid category IDs across distinct collection IDs.
    // collected=8 forces the set to appear as fully collected — no need for
    // MY COLLECTIONS to rely on AddEntry updating the count.
    let test_cases: &[(u16, u16, u32, u32, u8)] = &[
        // (collection_id, category_id, imageid, entry_count, collected)
        (1,     2, 5437, 8, 8),
        (2,     3, 5437, 8, 8),
        (3,     4, 5437, 8, 8),
        (4,     5, 5437, 8, 8),
    ];

    for &(cid, cat, imgid, count, collected) in test_cases {
        // blob = [id, category_id, imageid, entryCount] (4 × u32 LE)
        // blob[1] MUST be category_id — RefreshFn1 (EXE 0x007c2ec0) reads
        // [collection+0xa0] (= blob[1]) and compares it against the active
        // category filter. Wrong value here = collection invisible in UI.
        let _ = collected; // "collected" comes from AddEntry packets, not the blob
        let mut blob = Vec::with_capacity(16);
        blob.extend_from_slice(&(cid as u32).to_le_bytes());    // [collection+0x9c] = row key
        blob.extend_from_slice(&(cat as u32).to_le_bytes());    // [collection+0xa0] = category
        blob.extend_from_slice(&imgid.to_le_bytes());            // [collection+0xa4] = imageid
        blob.extend_from_slice(&(count as u32).to_le_bytes());  // [collection+0xa8] = entryCount

        pkts.push(GamePacket::serialize(&TunneledPacket {
            unknown1: true,
            inner: CollectionStart {
                collection_id: cid,
                unknown1: cat,
                blob,
            },
        }));

        // CollectionAddEntry: one entry per slot, last slot sets is_complete=true
        for slot in 0..count {
            pkts.push(GamePacket::serialize(&TunneledPacket {
                unknown1: true,
                inner: CollectionAddEntry {
                    collection_id: cid,
                    slot: slot as u16,
                    item_name_id: 1000 + slot, // placeholder name IDs
                    log_field1: 0,
                    log_field2: 0,
                    unknown4: 0,
                    unknown5: 0,
                    unknown6: 0,
                    unknown7: 0,
                    is_complete: slot == count - 1,
                },
            }));
        }
    }

    pkts
}

pub fn make_test_nameplate_image(guid: u32) -> Vec<Vec<u8>> {
    vec![GamePacket::serialize(&TunneledPacket {
        unknown1: true,
        inner: NameplateImageId {
            image_id: NameplateImage::Trooper,
            guid: player_guid(guid),
        },
    })]
}
