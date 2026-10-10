use std::io::Cursor;

use rand::seq::SliceRandom;

use packet_serialize::DeserializePacket;

use std::collections::{HashMap, HashSet};

use crate::game_server::{
    packets::{
        item::{Attachment, BaseAttachmentGroup, EquipmentSlot, WieldType},
        pet::{PetEquipAttachment, PetInventory},
        player_data::{BattleClassItem, Item2, ItemGuid, Pet, PetTrick, Unknown12, Unknown13, Unknown2},
        player_update::{SetAnimation, AddCompositeEffectTag, AddNpc, Hostility, RemoveCompositeEffectTag, Icon, NpcRelevance, PhysicsState, PlayCompositeEffect, QueueAnimation, SingleNpcRelevance, RemoveGracefully, UpdateEquippedItem,
            UpdateIdleAnimation, UpdateRunAnimation, UpdateWalkAnimation},
        tunnel::TunneledPacket,
        ui::ExecuteScriptWithIntParams,
        update_position::UpdatePlayerPos,
        GamePacket, Pos, Target,
    },
    Broadcast, GameServer, ProcessPacketError, ProcessPacketErrorType,
};

use super::{
    character::{Character, CharacterType},
    guid::{GuidTableIndexer, IndexedGuid},
    lock_enforcer::CharacterLockRequest,
    unique_guid::{pet_guid, player_guid},
    zone::ZoneInstance,
};

/// Maps companion item_guid → attachment item group ID.
/// These are the item groups that open in the flyout when the player clicks the
/// gear icon in ActivePetWindow.  Group IDs must be real GEAR groups present in
/// item_groups.yaml whose items are companion parts — NOT the purchase group for
/// the companion itself.  Groups with multiple part packs use the base pack here;
/// the category buttons in the flyout (Task #25) will let players switch packs.
/// 0 means no gear (gear icon stays grayed out; SetPetAttachmentItemGroupId is
/// skipped entirely to avoid hiding the window).
/// All group IDs here must be REAL client-side item group IDs (from item_groups.yaml /
/// the client's local data).  Fake aggregate groups (10001-10006) do not exist in the
/// client's item group table, so the flyout will show nothing if you use them here.
///
/// For companions whose attachments span multiple packs (protocol droids, astromechs,
/// etc.), this uses the BASE pack group — the one that contains the core attachment items.
/// DLC packs (groups 512, 513, 530, 549, …) are not shown yet; category-tab support
/// (task #25) will add a way to switch between them.
pub static COMPANION_ATTACHMENT_GROUPS: &[(u32, u32)] = &[
    // Protocol droids → base Protocol Droid Part Pack (175: items 311/430/447)
    (541,  175), // C-3PO
    (550,  175), // N0-80T
    (551,  175), // N-30H
    (552,  175), // J3-3V3
    (553,  175), // D-0T
    (998,  175), // RA-7
    // Techno-Service droid → Techno-Service Droid Part Pack (172, only one group)
    (633,  172), // T0-D0
    // Astromech droids → group 174 (base pack)
    (545,  174), // R2-D2
    (546,  174), // R3-S6
    (547,  174), // B3-3P5
    (548,  174), // M1-L0
    (549,  174), // B0-LT5
    (916,  174), // R2-KT
    (917,  174), // B3-T4
    (2146, 174), // 6R-0WL (Wampa R2)
    (2147, 174), // P3-NUT (Nutcracker R2)
    (2393, 174), // H3-4RT (Valentine R2)
    (2804, 174), // 7L-VN (711 promo R2)
    (3408, 174), // M5-BZ
    (3465, 174), // U9-C4
    // LEP servant droids → LEP Servant Droid parts (group 170: 445 Hologram Projector, 446 Sparring Datacard)
    (554,  170), // BR-3R
    (3466, 170), // BNI-393
    (3468, 174), // L1-LR3D
    // Mouse droids → base Mouse Droid Part Pack (168: items 424/444)
    (542,  168), // F1-V3L
    (2092, 168), // 1M-AU5 (gold mouse droid)
    // Probe droid → Probe Droid Parts (171)
    (543,  171), // PR-0B07
    // Power droid → base Power Droid Part Pack (173: items 428/429/436)
    (544,  173), // 5T-U85
    // AT-AT → AT-AT Datacard Part Pack (169: items 563=Spinners, 564=Bipedal Datacard)
    // Group 713 (item 2713=Puppy) is a separate pack shown via category tabs (task #25).
    (632,  169), // 5P-0T
    // Creatures with actual gear groups (all others have no gear; gear icon stays gray)
    (2752, 641), // Aedalus  → Convor Gear (group 641: items 2715/2716)
    (2802, 837), // Eeetch   → Kowakian Monkey Lizard Costumes (group 837: 3274/3275) + Fez & Cymbals 3288
    (3460, 837), // Skreech  → same Kowakian gear
    (3406, 837), // Cercopes → same Kowakian gear
    (3282, 843), // Phileas  → Balloon Attachment (group 843: item 3285) + Mon Calamari Hat 3289
    // Companions added 3470-3490
    (3470, 641), // Bianca
    (3471, 641), // Cairo
    (3472, 641), // Tully
    (3477, 837), // Ceith
    (3478, 837), // Syther
    (3486, 174), // R4-T1
    (3487, 174), // R4-YL0
    (3488, 174), // R4-LT
    (3489, 174), // R4-L9
    (3490, 174), // R4-PRP
];

/// Maps companion item_guid → the item group ID to pass as `BaseAttachmentGroup.unknown1`
/// in AddNpc for the DEFAULT animation variant (i.e. what the companion uses when first
/// spawned with no attachment explicitly equipped by the player).
///
/// This is DISTINCT from `COMPANION_ATTACHMENT_GROUPS` (which controls the gear-icon
/// flyout via `SetPetAttachmentItemGroupId`).  `BaseAttachmentGroup.unknown1` in AddNpc
/// tells the client which item group to look up in the player's inventory to select a
/// `Loc_Attach*` animation variant:
///   - 0      → no attachment override, use base animations (e.g. quadruped for AT-AT)
///   - 169    → items 563 (Bipedal Datacard) and 564 (Spinners) for AT-AT
///   - 713    → item 2713 (Puppy) for AT-AT
///
/// When the player has item 563 in their inventory AND the group is 169, the client
/// activates `Loc_Attach1` (bipedal walk) for 5P-0T.  Test players always have 563
/// in inventory, so this gives bipedal by default.  Profiled players need to own 563.
///
/// Once per-companion equipped-attachment tracking is implemented, the spawn will use
/// the player's actual equipped attachment group instead of this static default.
/// NOTE: `BaseAttachmentGroup.unknown1` in AddNpc does NOT appear to directly
/// control the companion's walk animation variant.  The Loc_Attach* animations
/// (e.g. bipedal for AT-AT) are likely triggered by a `PetEquipPacket` that the
/// client sends when the player clicks an item in the attachment flyout, and that
/// the server must echo back.  The format of that packet is still unknown.
///
/// The mapping below is kept for reference; passing group 169 in AddNpc does not
/// change the AT-AT's gait.  Once the equip packet format is captured we can
/// persist the equipped attachment in the player profile and replay it on spawn.
pub static COMPANION_ANIMATION_GROUPS: &[(u32, u32)] = &[
    // AT-AT: group 169 items confirmed from game log:
    //   563 → icon 254  → "Spinners"         → Loc_Attach2 (decorative spinners, 4-legged gait)
    //   564 → icon 1182 → "Bipedal Datacard" → Loc_Attach1 (bipedal 2-legged walk)
    // Passing group 169 in AddNpc currently has no observed effect on animation.
    // TODO: remove or repurpose once PetEquipPacket is implemented.
    (632, 169), // 5P-0T
    // All other companions use base animations (0) until the equip packet is understood.
];


/// Maps companion item_guid → its attachment items, as `(item_guid, unused, icon_set_id)`.
/// Used to grant attachment items to the inventory at login.  (Which attachments a
/// companion accepts in the flyout comes from `COMPANION_PET_DATA`.)
pub static COMPANION_ATTACHMENT_ITEMS: &[(u32, &[(u32, u32, u32)])] = &[
    // Protocol droids → groups 175/512/530/550
    // 311=Serving Tray/266, 430=Oil Slick/1184, 447=Dance/1183,
    // 2109=1398, 425=267, 559=1431, 2156=1515
    (541,  &[(311, 0, 266),(430, 0, 1184),(447, 0, 1183),(2109, 0, 1398),(425, 0, 267),(559, 0, 1431),(2156, 0, 1515)]),  // C-3PO
    (550,  &[(311, 0, 266),(430, 0, 1184),(447, 0, 1183),(2109, 0, 1398),(425, 0, 267),(559, 0, 1431),(2156, 0, 1515)]),  // N0-80T
    (551,  &[(311, 0, 266),(430, 0, 1184),(447, 0, 1183),(2109, 0, 1398),(425, 0, 267),(559, 0, 1431),(2156, 0, 1515)]),  // N-30H
    (552,  &[(311, 0, 266),(430, 0, 1184),(447, 0, 1183),(2109, 0, 1398),(425, 0, 267),(559, 0, 1431),(2156, 0, 1515)]),  // J3-3V3
    (553,  &[(311, 0, 266),(430, 0, 1184),(447, 0, 1183),(2109, 0, 1398),(425, 0, 267),(559, 0, 1431),(2156, 0, 1515)]),  // D-0T
    (998,  &[(311, 0, 266),(430, 0, 1184),(447, 0, 1183),(2109, 0, 1398),(425, 0, 267),(559, 0, 1431),(2156, 0, 1515)]),  // RA-7
    // Techno-Service droid → group 172: 560=1432, 561=269, 562=268
    (633,  &[(560, 0, 1432),(561, 0, 269),(562, 0, 268)]),  // T0-D0
    // Astromech droids → groups 174/513/529/549/635/727
    // 309=255, 310=1181, 427=258, 2085=1397, 2087=1394, 2088=1395, 2089=1396,
    // 438=257, 558=256, 2155=1511, 2205=1512, 2714=1800, 2903=2309, 2904=2310, 2905=2311
    (545,  &[(309, 0, 255),(310, 0, 1181),(427, 0, 258),(2085, 0, 1397),(2087, 0, 1394),(2088, 0, 1395),(2089, 0, 1396),(438, 0, 257),(558, 0, 256),(2155, 0, 1511),(2205, 0, 1512),(2714, 0, 1800),(2903, 0, 2309),(2904, 0, 2310),(2905, 0, 2311)]),  // R2-D2
    (546,  &[(309, 0, 255),(310, 0, 1181),(427, 0, 258),(2085, 0, 1397),(2087, 0, 1394),(2088, 0, 1395),(2089, 0, 1396),(438, 0, 257),(558, 0, 256),(2155, 0, 1511),(2205, 0, 1512),(2714, 0, 1800),(2903, 0, 2309),(2904, 0, 2310),(2905, 0, 2311)]),  // R3-S6
    (547,  &[(309, 0, 255),(310, 0, 1181),(427, 0, 258),(2085, 0, 1397),(2087, 0, 1394),(2088, 0, 1395),(2089, 0, 1396),(438, 0, 257),(558, 0, 256),(2155, 0, 1511),(2205, 0, 1512),(2714, 0, 1800),(2903, 0, 2309),(2904, 0, 2310),(2905, 0, 2311)]),  // B3-3P5
    (548,  &[(309, 0, 255),(310, 0, 1181),(427, 0, 258),(2085, 0, 1397),(2087, 0, 1394),(2088, 0, 1395),(2089, 0, 1396),(438, 0, 257),(558, 0, 256),(2155, 0, 1511),(2205, 0, 1512),(2714, 0, 1800),(2903, 0, 2309),(2904, 0, 2310),(2905, 0, 2311)]),  // M1-L0
    (549,  &[(309, 0, 255),(310, 0, 1181),(427, 0, 258),(2085, 0, 1397),(2087, 0, 1394),(2088, 0, 1395),(2089, 0, 1396),(438, 0, 257),(558, 0, 256),(2155, 0, 1511),(2205, 0, 1512),(2714, 0, 1800),(2903, 0, 2309),(2904, 0, 2310),(2905, 0, 2311)]),  // B0-LT5
    (916,  &[(309, 0, 255),(310, 0, 1181),(427, 0, 258),(2085, 0, 1397),(2087, 0, 1394),(2088, 0, 1395),(2089, 0, 1396),(438, 0, 257),(558, 0, 256),(2155, 0, 1511),(2205, 0, 1512),(2714, 0, 1800),(2903, 0, 2309),(2904, 0, 2310),(2905, 0, 2311)]),  // R2-KT
    (917,  &[(309, 0, 255),(310, 0, 1181),(427, 0, 258),(2085, 0, 1397),(2087, 0, 1394),(2088, 0, 1395),(2089, 0, 1396),(438, 0, 257),(558, 0, 256),(2155, 0, 1511),(2205, 0, 1512),(2714, 0, 1800),(2903, 0, 2309),(2904, 0, 2310),(2905, 0, 2311)]),  // B3-T4
    (2146, &[(309, 0, 255),(310, 0, 1181),(427, 0, 258),(2085, 0, 1397),(2087, 0, 1394),(2088, 0, 1395),(2089, 0, 1396),(438, 0, 257),(558, 0, 256),(2155, 0, 1511),(2205, 0, 1512),(2714, 0, 1800),(2903, 0, 2309),(2904, 0, 2310),(2905, 0, 2311)]),  // 6R-0WL
    (2147, &[(309, 0, 255),(310, 0, 1181),(427, 0, 258),(2085, 0, 1397),(2087, 0, 1394),(2088, 0, 1395),(2089, 0, 1396),(438, 0, 257),(558, 0, 256),(2155, 0, 1511),(2205, 0, 1512),(2714, 0, 1800),(2903, 0, 2309),(2904, 0, 2310),(2905, 0, 2311)]),  // P3-NUT
    (2393, &[(309, 0, 255),(310, 0, 1181),(427, 0, 258),(2085, 0, 1397),(2087, 0, 1394),(2088, 0, 1395),(2089, 0, 1396),(438, 0, 257),(558, 0, 256),(2155, 0, 1511),(2205, 0, 1512),(2714, 0, 1800),(2903, 0, 2309),(2904, 0, 2310),(2905, 0, 2311)]),  // H3-4RT
    (2804, &[(309, 0, 255),(310, 0, 1181),(427, 0, 258),(2085, 0, 1397),(2087, 0, 1394),(2088, 0, 1395),(2089, 0, 1396),(438, 0, 257),(558, 0, 256),(2155, 0, 1511),(2205, 0, 1512),(2714, 0, 1800),(2903, 0, 2309),(2904, 0, 2310),(2905, 0, 2311)]),  // 7L-VN
    (3408, &[(309, 0, 255),(310, 0, 1181),(427, 0, 258),(2085, 0, 1397),(2087, 0, 1394),(2088, 0, 1395),(2089, 0, 1396),(438, 0, 257),(558, 0, 256),(2155, 0, 1511),(2205, 0, 1512),(2714, 0, 1800),(2903, 0, 2309),(2904, 0, 2310),(2905, 0, 2311)]),  // M5-BZ
    (3465, &[(309, 0, 255),(310, 0, 1181),(427, 0, 258),(2085, 0, 1397),(2087, 0, 1394),(2088, 0, 1395),(2089, 0, 1396),(438, 0, 257),(558, 0, 256),(2155, 0, 1511),(2205, 0, 1512),(2714, 0, 1800),(2903, 0, 2309),(2904, 0, 2310),(2905, 0, 2311)]),  // U9-C4
    (3468, &[(309, 0, 255),(310, 0, 1181),(427, 0, 258),(2085, 0, 1397),(2087, 0, 1394),(2088, 0, 1395),(2089, 0, 1396),(438, 0, 257),(558, 0, 256),(2155, 0, 1511),(2205, 0, 1512),(2714, 0, 1800),(2903, 0, 2309),(2904, 0, 2310),(2905, 0, 2311)]),  // L1-LR3D
    // Mouse droids → groups 168/551/609/682: 424=262, 444=192, 2157=1513, 2158=1514, 2381=1677, 2819=2011
    (542,  &[(424, 0, 262),(444, 0, 192),(2157, 0, 1513),(2158, 0, 1514),(2381, 0, 1677),(2819, 0, 2011)]),  // F1-V3L
    (2092, &[(424, 0, 262),(444, 0, 192),(2157, 0, 1513),(2158, 0, 1514),(2381, 0, 1677),(2819, 0, 2011)]),  // 1M-AU5
    // Probe droid → group 171: 657=1429, 658=1427, 659=1428
    (543,  &[(657, 0, 1429),(658, 0, 1427),(659, 0, 1428)]),  // PR-0B07
    // LEP servant droids → group 170: 445=259, 446=260
    (554,  &[(445, 0, 259),(446, 0, 260)]),  // BR-3R
    (3466, &[(445, 0, 259),(446, 0, 260)]),  // BNI-393
    // Power droid → groups 173/610: 428=265, 429=264, 436=263, 2382=1676
    (544,  &[(428, 0, 265),(429, 0, 264),(436, 0, 263),(2382, 0, 1676)]),  // 5T-U85
    // Mini AT-AT → groups 169/713: 563=254, 564=1182, 2713=1801
    (632,  &[(563, 0, 254),(564, 0, 1182),(2713, 0, 1801)]),  // 5P-0T
    // Aedalus (Convor) → group 641: 2715=1847, 2716=1848
    (2752, &[(2715, 0, 1847),(2716, 0, 1848)]),  // Aedalus
    // Eeetch (Kowakian) → group 837: 3274=2526, 3275=2527
    (2802, &[(3274, 0, 2526),(3275, 0, 2527),(3288, 0, 2829)]),  // Eeetch
    (3460, &[(3274, 0, 2526),(3275, 0, 2527),(3288, 0, 2829)]),  // Skreech
    (3406, &[(3274, 0, 2526),(3275, 0, 2527),(3288, 0, 2829)]),  // Cercopes
    // Phileas (Sarlacc) → group 843: 3285=2575
    (3282, &[(3285, 0, 2575),(3289, 0, 2926)]),  // Phileas
    // Companions added 3470-3490
    (3470, &[(2715, 0, 1847),(2716, 0, 1848)]),  // Bianca
    (3471, &[(2715, 0, 1847),(2716, 0, 1848)]),  // Cairo
    (3472, &[(2715, 0, 1847),(2716, 0, 1848)]),  // Tully
    (3477, &[(3274, 0, 2526),(3275, 0, 2527),(3288, 0, 2829)]),  // Ceith
    (3478, &[(3274, 0, 2526),(3275, 0, 2527),(3288, 0, 2829)]),  // Syther
    (3486, &[(309, 0, 255),(310, 0, 1181),(427, 0, 258),(2085, 0, 1397),(2087, 0, 1394),(2088, 0, 1395),(2089, 0, 1396),(438, 0, 257),(558, 0, 256),(2155, 0, 1511),(2205, 0, 1512),(2714, 0, 1800),(2903, 0, 2309),(2904, 0, 2310),(2905, 0, 2311)]),  // R4-T1
    (3487, &[(309, 0, 255),(310, 0, 1181),(427, 0, 258),(2085, 0, 1397),(2087, 0, 1394),(2088, 0, 1395),(2089, 0, 1396),(438, 0, 257),(558, 0, 256),(2155, 0, 1511),(2205, 0, 1512),(2714, 0, 1800),(2903, 0, 2309),(2904, 0, 2310),(2905, 0, 2311)]),  // R4-YL0
    (3488, &[(309, 0, 255),(310, 0, 1181),(427, 0, 258),(2085, 0, 1397),(2087, 0, 1394),(2088, 0, 1395),(2089, 0, 1396),(438, 0, 257),(558, 0, 256),(2155, 0, 1511),(2205, 0, 1512),(2714, 0, 1800),(2903, 0, 2309),(2904, 0, 2310),(2905, 0, 2311)]),  // R4-LT
    (3489, &[(309, 0, 255),(310, 0, 1181),(427, 0, 258),(2085, 0, 1397),(2087, 0, 1394),(2088, 0, 1395),(2089, 0, 1396),(438, 0, 257),(558, 0, 256),(2155, 0, 1511),(2205, 0, 1512),(2714, 0, 1800),(2903, 0, 2309),(2904, 0, 2310),(2905, 0, 2311)]),  // R4-L9
    (3490, &[(309, 0, 255),(310, 0, 1181),(427, 0, 258),(2085, 0, 1397),(2087, 0, 1394),(2088, 0, 1395),(2089, 0, 1396),(438, 0, 257),(558, 0, 256),(2155, 0, 1511),(2205, 0, 1512),(2714, 0, 1800),(2903, 0, 2309),(2904, 0, 2310),(2905, 0, 2311)]),  // R4-PRP
];

/// Maps companion item_guid → NPC model_id.
///
/// All `.adr` companion models must be wrapped in a `Grp_Companion_*.agr` file with
/// `validatePcNpc="0"` (like Grp_ATAT.agr) so the client renders them as NPCs instead
/// of validating them as PC models (which silently fails → invisible companion).
/// These wrapper files live as loose files in the CWA game directory and their
/// model IDs are registered in the loose Models.txt (entries 2834+).
pub static COMPANION_MODELS: &[(u32, u32)] = &[
    // Protocol droids
    (541,  2835), // C-3PO              (Grp_Companion_ProtocolDroid3PO.agr → Char_ProtocolDroid3PO.adr)
    (550,  644),  // N0-80T             (3D_TC14.agr — companion agr, validatePcNpc=0)
    (551,  646),  // N-30H              (3D_TC3.agr)
    (552,  647),  // J3-3V3             (3D_TC70.agr)
    (553,  645),  // D-0T               (3D_TC2.agr)
    (633,  2841), // T0-D0              (Grp_Companion_TechnoServiceDroid.agr → Char_TechnoServiceDroid.adr)
    (998,  2843), // RA-7               (Grp_Companion_DeathStarDroidR7.agr → Char_DeathStarDroidR7.adr)
    // Astromech droids
    (545,  2834), // R2-D2              (Grp_Companion_AstroMechR2.agr → Char_AstroMechR2.adr)
    (546,  642),  // R3-S6              (3D_R3S6.agr)
    (547,  2838), // B3-3P5             (Grp_Companion_AstroMechR4.agr → Char_AstroMechR4.adr)
    (548,  643),  // M1-L0              (3D_R4J1.agr)
    (549,  2839), // B0-LT5             (Grp_Companion_AstroMechR5.agr → Char_AstroMechR5.adr)
    (916,  2220), // R2-KT              (3D_R2KT.agr)
    (917,  1104), // B3-T4              (3D_B3T4.agr)
    (2146, 1227), // 6R-0WL             (Grp_R2_Wampa.agr — companion agr, validatePcNpc=0)
    (2147, 1319), // P3-NUT             (Grp_R2_Nutcracker.agr)
    (2393, 1408), // H3-4RT             (Grp_R2_Valentine.agr)
    (2804, 1622), // 7L-VN              (Grp_R2_711Promo.agr)
    // Mouse droids
    (542,  2837), // F1-V3L             (Grp_Companion_MouseDroid.agr → Char_MouseDroid.adr)
    (2092, 2844), // 1M-AU5             (Grp_Companion_MouseDroidGold.agr → Char_MouseDroidGold.adr)
    // Probe & power droids
    (543,  2891), // PR-0B07            (Grp_Companion_ProbeDroid.agr → Char_ProbeDroid.adr, Blue — has the Jurassic / Missile / Saucer Loc_Attach sets; the _TD model does not)
    (544,  2836), // 5T-U85             (Grp_Companion_PowerDroid.agr → Char_PowerDroid.adr)
    // Other droids
    (554,  2840), // BR-3R              (Grp_Companion_LEPServantDroid.agr → Char_LEPServantDroid.adr)
    (632,  2833), // 5P-0T              (Grp_ATAT.agr → Char_ATAT.adr, validatePcNpc=0)
    (2114, 1142), // J-4CK              (Grp_R4_Pumpkin.agr)
    (2115, 1141), // K-3RNL             (Grp_R4_CandyCorn.agr)
    (3080, 2849), // DU-3               (Grp_Companion_PitDroid.agr → Char_PitDroid.adr)
    (3081, 2850), // K0-5D              (Grp_Companion_GladiatorDroid.agr → Char_GladiatorDroid.adr)
    (3284, 2236), // BR-RR              (3D_BRRR.agr)
    // Creatures
    (2752, 2845), // Aedalus            (Grp_Companion_Convor.agr → Char_Convor.adr)
    (2786, 2846), // Gnarls             (Grp_Companion_Anoobus.agr → Char_Anoobus_LOD0.adr)
    (2802, 2847), // Eeetch             (Grp_Companion_KowakianMonkeyLizard.agr → Char_KowakianMonkeyLizard.adr, default Red)
    (2899, 1737), // Vector             (3D_Vector.agr)
    (3073, 1944), // Babbo              (3D_Babbo.agr — field4 patched to 1.0 in loose Models.txt)
    (3074, 1945), // Duppa              (3D_Duppa.agr — field4 patched to 1.0 in loose Models.txt)
    (3100, 2854), // Fang               (Grp_Companion_Rancor.agr → Char_Rancor.adr)
    (3219, 2161), // Erial              (3D_Convor_Purple.agr)
    (3282, 2852), // Phileas            (Grp_Companion_Sarlacc.agr → Char_Sarlacc.adr)
    (3283, 2853), // Barnibus           (Grp_Companion_Rancor_Glow.agr → Char_Rancor_Glow.adr)
    (3366, 2293), // Marrok             (Char_Marrok.adr — TODO: needs wrapper if invisible)
    (3373, 2299), // Ghorroh            (Char_Gundark.adr — TODO: needs wrapper if invisible)
    // Sidekicks
    (3079, 2848), // Aktik              (Grp_Companion_Holo_Jawa.agr → Holo_Jawa.adr)
    (3341, 2851), // Jynx               (Grp_Companion_Holo_NPCGungan.agr → Holo_NPCGungan.adr)
    (3398, 2855), // Klug               (Grp_Companion_Holo_GamorreanGuard.agr → Holo_GamorreanGuard.adr)
    // Missing companions (assigned GUIDs 3406-3468; 3420-3447 were original furniture/set ids, moved to 3460-3468)
    (3460, 2861), // Skreech            (Grp_Companion_KowakianMonkeyLizard_Blue.agr — blue Kowakian)
    (3461, 2865), // Cinch              (Grp_Companion_Anacondan.agr → Char_Anacondan.adr, Morley)
    (3462, 2182), // Amelie             (Char_Millicreep.adr — TODO: needs wrapper if invisible)
    (3463, 2470), // Gulsh              (Char_Narglatch.adr — TODO: needs wrapper if invisible)
    (3464, 2863), // Kharroh            (Grp_Companion_Gundark_Green.agr — green Gundark)
    (3415, 2864), // Gash               (Grp_Companion_Narglatch_Green.agr — green Narglatch)
    (3406, 2862), // Cercopes           (Grp_Companion_KowakianMonkeyLizard_Green.agr — green Kowakian)
    (3407, 2858), // WAC-47             (Grp_Companion_PitDroidAsym.agr → Char_PitDroidAsym.adr)
    (3408, 2866), // M5-BZ              (Grp_Companion_AstroMechR4_M5BZ.agr — R4 body, M5_BZ texture)
    (3465, 2664), // U9-C4              (3D_Astromech_U9C4.agr)
    (3466, 2859), // BNI-393            (Grp_Companion_LEPServantDroidBNI393.agr)
    (3467, 2857), // Security Battle Droid (Grp_Companion_BattleDroid.agr → Char_BattleDroid.adr)
    (3468, 641),  // L1-LR3D            (3D_R2M5.agr — white/red astromech, same as its marketplace preview)
    (3448, 2860), // H0DG3-P0DG3        (Grp_Companion_HodgepodgeDroid.agr → Char_HodgepodgeDroid.adr)
    // Companions added 3470-3490 (wrappers in the client folder, Models.txt 2867-2887)
    (3470, 2867), // Bianca             (Grp_Companion_Bianca.agr → Char_Convor.adr, White)
    (3471, 2868), // Cairo              (Grp_Companion_Cairo.agr → Char_Convor.adr, Blue)
    (3472, 2869), // Tully              (Grp_Companion_Tully.agr → Char_Convor.adr, Green)
    (3473, 2870), // Caldwell           (Grp_Companion_Caldwell.agr → Char_Monong.adr, TanGreen)
    (3474, 2871), // Mona               (Grp_Companion_Mona.agr → Char_Monong.adr, Ice)
    (3475, 2872), // Omma               (Grp_Companion_Omma.agr → Char_Monong.adr, Zanbar)
    (3476, 2873), // Oppa               (Grp_Companion_Oppa.agr → Char_Monong.adr, YellowBlack)
    (3477, 2874), // Ceith              (Grp_Companion_Ceith.agr → Char_KowakianMonkeyLizard.adr, Yellow)
    (3478, 2875), // Syther             (Grp_Companion_Syther.agr → Char_KowakianMonkeyLizard.adr, Black)
    (3479, 2876), // Fogg               (Grp_Companion_Fogg.agr → Char_Rancor_Glow.adr, Blue)
    (3480, 2877), // Minoan             (Grp_Companion_Minoan.agr → Char_Rancor_Combat_Small.adr, Zanbar)
    (3481, 2878), // Halcombe           (Grp_Companion_Halcombe.agr → Char_Gundark.adr, BlueSilver)
    (3482, 2879), // Rosie              (Grp_Companion_Rosie.agr → Char_Tooka.adr, Magenta)
    (3483, 2880), // Tardar             (Grp_Companion_Tardar.agr → Char_Tooka.adr, PurpleGreen)
    (3484, 2881), // Tooka              (Grp_Companion_TookaGray.agr → Char_Tooka.adr, Zanbar)
    (3485, 2882), // Zanbar             (Grp_Companion_Zanbar.agr → Char_Anoobus_LOD0.adr, AnoobusZanbar)
    (3486, 2883), // R4-T1              (Grp_Companion_R4T1.agr → Char_AstroMechR4.adr, R4RD)
    (3487, 2884), // R4-YL0             (Grp_Companion_R4YL0.agr → Char_AstroMechR4.adr, R4YL0)
    (3488, 2885), // R4-LT              (Grp_Companion_R4LT.agr → Char_AstroMechR4.adr, R4TL)
    (3489, 2886), // R4-L9              (Grp_Companion_R4L9.agr → Char_AstroMechR4.adr, R4BL9)
    (3490, 2887), // R4-PRP             (Grp_Companion_R4PRP.agr → Char_AstroMechR4.adr, R4PRP)
];

/// Returns true if `item_guid` is a known companion (one that can be summoned).
/// Used by the store to ensure purchased companions are added to `player.companions`.
pub fn is_companion_item(item_guid: u32) -> bool {
    COMPANION_MODELS.iter().any(|&(g, _)| g == item_guid)
}

/// Display metadata for every companion.
/// Format: (item_guid, display_name, icon_set_id, attachment_group_guid, all_possible_attachment_guids)
///
/// `icon_set_id`          — shown in PetListWindow for this companion.
/// `attachment_group_guid`— stored in Pet.unknown12.unknown1; C++ uses it to route
///                          PetInventory DataSource updates (0 = no gear slots).
/// `all_possible_attachment_guids` — all attachment items this companion type can use.
///   `build_pet_for_companion` filters these to only items the player actually owns.
pub static COMPANION_PET_DATA: &[(u32, &str, u32, u32, &[u32])] = &[
    // Protocol droids (base group 175: items 311/430/447)
    (541,  "C-3PO",                  74,   175, &[311, 430, 447, 2109, 425, 559, 2156]),
    (550,  "N0-80T",                 77,   175, &[311, 430, 447, 2109, 425, 559, 2156]),
    (551,  "N-30H",                  78,   175, &[311, 430, 447, 2109, 425, 559, 2156]),
    (552,  "J3-3V3",                 76,   175, &[311, 430, 447, 2109, 425, 559, 2156]),
    (553,  "D-0T",                   75,   175, &[311, 430, 447, 2109, 425, 559, 2156]),
    (998,  "RA-7",                   1231, 175, &[311, 430, 447, 2109, 425, 559, 2156]),
    // Mouse droids (base group 168: items 424/444)
    (542,  "F1-V3L",                 71,   168, &[424, 444, 2157, 2158, 2381, 2819]),
    (2092, "1M-AU5",                 1258, 168, &[424, 444, 2157, 2158, 2381, 2819]),
    // Probe droids (group 171, no aggregate)
    (543,  "PR-0B07",                73,   171, &[657, 658, 659]),
    // Power droids (base group 173: items 428/429/436)
    (544,  "5T-U85",                 72,   173, &[428, 429, 436, 2382]),
    // Astromech droids (group 174 — base pack + all DLC parts added to it)
    (545,  "R2-D2",                  64,   174, &[309, 310, 427, 2085, 2087, 2088, 2089, 438, 558, 2155, 2205, 2714, 2903, 2904, 2905]),
    (546,  "R3-S6",                  66,   174, &[309, 310, 427, 2085, 2087, 2088, 2089, 438, 558, 2155, 2205, 2714, 2903, 2904, 2905]),
    (547,  "B3-3P5",                 67,   174, &[309, 310, 427, 2085, 2087, 2088, 2089, 438, 558, 2155, 2205, 2714, 2903, 2904, 2905]),
    (548,  "M1-L0",                  68,   174, &[309, 310, 427, 2085, 2087, 2088, 2089, 438, 558, 2155, 2205, 2714, 2903, 2904, 2905]),
    (549,  "B0-LT5",                 69,   174, &[309, 310, 427, 2085, 2087, 2088, 2089, 438, 558, 2155, 2205, 2714, 2903, 2904, 2905]),
    (916,  "R2-KT",                  1075, 174, &[309, 310, 427, 2085, 2087, 2088, 2089, 438, 558, 2155, 2205, 2714, 2903, 2904, 2905]),
    (917,  "B3-T4",                  1035, 174, &[309, 310, 427, 2085, 2087, 2088, 2089, 438, 558, 2155, 2205, 2714, 2903, 2904, 2905]),
    (2146, "6R-0WL",                 1464, 174, &[309, 310, 427, 2085, 2087, 2088, 2089, 438, 558, 2155, 2205, 2714, 2903, 2904, 2905]),
    (2147, "P3-NUT",                 1465, 174, &[309, 310, 427, 2085, 2087, 2088, 2089, 438, 558, 2155, 2205, 2714, 2903, 2904, 2905]),
    (2393, "H3-4RT",                 1675, 174, &[309, 310, 427, 2085, 2087, 2088, 2089, 438, 558, 2155, 2205, 2714, 2903, 2904, 2905]),
    (2804, "7L-VN",                  1998, 174, &[309, 310, 427, 2085, 2087, 2088, 2089, 438, 558, 2155, 2205, 2714, 2903, 2904, 2905]),
    // Mini AT-AT → group 169 (items 563=Spinners, 564=Bipedal Datacard); Puppy in group 713
    (632,  "5P-0T",                  63,   169, &[563, 564, 2713]),
    // Techno-service droid (group 172)
    (633,  "T0-D0",                  79,   172, &[560, 561, 562]),
    // Round droids
    (554,  "BR-3R",                  70,   170, &[445, 446]),
    (2114, "J-4CK",                  1385, 0,   &[]),
    (2115, "K-3RNL",                 1384, 0,   &[]),
    (3080, "DU-3",                   2392, 0,   &[]),
    (3081, "K0-5D",                  2393, 0,   &[]),
    (3284, "BR-RR",                  2574, 0,   &[]),
    // Creatures
    (2752, "Aedalus",                1846, 641, &[2715, 2716]),
    (2786, "Gnarls",                 1976, 0,   &[]),
    (2802, "Eeetch",                 1997, 837, &[3274, 3275, 3288]),
    (2899, "Vector",                 2143, 0,   &[]),
    (3073, "Babbo",                  2357, 0,   &[]),
    (3074, "Duppa",                  2358, 0,   &[]),
    (3100, "Fang",                   2587, 0,   &[]),
    (3219, "Erial",                  2463, 0,   &[]),
    (3282, "Phileas",                2572, 843, &[3285, 3289]),
    (3283, "Barnibus",               2573, 0,   &[]),
    (3366, "Marrok",                 2667, 0,   &[]),
    (3373, "Ghorroh",                2700, 0,   &[]),
    // Missing companions
    (3460, "Skreech",                2922, 837, &[3274, 3275, 3288]),
    (3461, "Cinch",                  2927, 0,   &[]),
    (3462, "Amelie",                 2621, 0,   &[]),
    (3463, "Gulsh",                  2952, 0,   &[]),
    (3464, "Kharroh",                2995, 0,   &[]),
    (3415, "Gash",                   3132, 0,   &[]),
    (3406, "Cercopes",               3311, 837, &[3274, 3275, 3288]),
    (3407, "WAC-47",                 3260, 0,   &[]),
    (3408, "M5-BZ",                  3265, 174, &[309, 310, 427, 2085, 2087, 2088, 2089, 438, 558, 2155, 2205, 2714, 2903, 2904, 2905]),
    (3465, "U9-C4",                  3313, 174, &[309, 310, 427, 2085, 2087, 2088, 2089, 438, 558, 2155, 2205, 2714, 2903, 2904, 2905]),
    (3466, "BNI-393",                3341, 170, &[445, 446]),
    (3467, "Security Battle Droid",  3691, 0,   &[]),
    (3468, "L1-LR3D",                65,   174, &[309, 310, 427, 2085, 2087, 2088, 2089, 438, 558, 2155, 2205, 2714, 2903, 2904, 2905]),
    (3448, "H0DG3-P0DG3",            3873, 0,   &[]),
    // Sidekicks
    (3079, "Aktik",                  1961, 0,   &[]),
    (3341, "Jynx",                   1341, 0,   &[]),
    (3398, "Klug",                   2131, 0,   &[]),
    // Companions added 3470-3490
    (3470, "Bianca",               3874, 641, &[2715, 2716]),
    (3471, "Cairo",                3875, 641, &[2715, 2716]),
    (3472, "Tully",                3876, 641, &[2715, 2716]),
    (3473, "Caldwell",             2360, 0, &[]),
    (3474, "Mona",                 2359, 0, &[]),
    (3475, "Omma",                 3636, 0, &[]),
    (3476, "Oppa",                 2361, 0, &[]),
    (3477, "Ceith",                3877, 837, &[3274, 3275, 3288]),
    (3478, "Syther",               3878, 837, &[3274, 3275, 3288]),
    (3479, "Fogg",                 2642, 0, &[]),
    (3480, "Minoan",               3654, 0, &[]),
    (3481, "Halcombe",             3135, 0, &[]),
    (3482, "Rosie",                3879, 0, &[]),
    (3483, "Tardar",               3694, 0, &[]),
    (3484, "Tooka",                3820, 0, &[]),
    (3485, "Zanbar",               3625, 0, &[]),
    (3486, "R4-T1",                3863, 174, &[309, 310, 427, 2085, 2087, 2088, 2089, 438, 558, 2155, 2205, 2714, 2903, 2904, 2905]),
    (3487, "R4-YL0",               3865, 174, &[309, 310, 427, 2085, 2087, 2088, 2089, 438, 558, 2155, 2205, 2714, 2903, 2904, 2905]),
    (3488, "R4-LT",                3864, 174, &[309, 310, 427, 2085, 2087, 2088, 2089, 438, 558, 2155, 2205, 2714, 2903, 2904, 2905]),
    (3489, "R4-L9",                3861, 174, &[309, 310, 427, 2085, 2087, 2088, 2089, 438, 558, 2155, 2205, 2714, 2903, 2904, 2905]),
    (3490, "R4-PRP",               3862, 174, &[309, 310, 427, 2085, 2087, 2088, 2089, 438, 558, 2155, 2205, 2714, 2903, 2904, 2905]),
];

/// Builds a `Pet` struct for `companion_guid` using `COMPANION_PET_DATA`.
/// Returns `None` if the guid has no entry in the table.
///
/// Field notes (client record layout, parser 0x78af80):
///  - `item_guid` (+0xbc) is the pet's EQUIPPED item list: an inventory item
///    counts as equipped when its guid is in it.  Only the companion itself for now.
///  - `battle_class_items` (+0xe4) is keyed by the `item_class` of every
///    attachment this companion accepts; the attachment flyout filter requires it.
///  - `unknown10` (+0x110) = PetList "Is Vanity Pet",
///    `unknown11` (+0x114) = PetList "Attachment ItemGroup ID" (gear icon).
///
/// `owned_items` is currently unused.
pub fn build_pet_for_companion(companion_guid: u32, owned_items: &HashSet<u32>) -> Option<Pet> {
    let &(_, name, icon_id, attachment_group, all_attachments) = COMPANION_PET_DATA
        .iter()
        .find(|&&(g, _, _, _, _)| g == companion_guid)?;

    let _ = owned_items;
    let item_guid = vec![ItemGuid { guid: companion_guid }];

    Some(Pet {
        pet_id: companion_guid,
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
        // Attachment item defs use item_class = their own guid
        // (droid_parts.yaml / creature_gear.yaml), so key by attachment guid.
        battle_class_items: all_attachments
            .iter()
            .map(|&key| BattleClassItem {
                item1: key,
                item2: Item2 { unknown1: 0, unknown2: 0 },
            })
            .collect(),
        pet_name: name.to_string(),
        tint_id: 0,
        texture_alias: String::new(),
        icon_id,
        // BaseClient.PetList columns (client GetData at 0xb74d50):
        //   "Is Vanity Pet" = record +0x110 = unknown10,
        //   "Attachment ItemGroup ID" = record +0x114 = unknown11.
        // ActivePetWindow.SetActivePetId reads the latter to enable the gear icon.
        unknown10: true,
        unknown11: attachment_group,
        unknown12: Unknown12 {
            unknown1: attachment_group,
            unknown2: 0,
            unknown3: 0,
            unknown4: 0,
        },
        unknown13: Unknown13 {
            unknown1: 0, unknown2: 0, unknown3: 0, unknown4: 0,
            unknown5: 0, unknown6: 0, unknown7: 0, unknown8: 0,
        },
    })
}

/// The player's pet records as sent at login (unknown2 = true), with `active_pet`
/// guaranteed present.  PetInventory REPLACES the client's whole pet list (which
/// also backs BaseClient.PetList used by the HUD/pet list), so every owned pet
/// must be included, not just the active one.
///
/// `equipped` (companion → attachment) adds each equipped attachment to that
/// pet's `item_guid` list, which the client treats as the pet's equipped items.
pub fn owned_pet_records(
    active_pet: u32,
    owned_items: &HashSet<u32>,
    equipped: &HashMap<u32, u32>,
) -> Vec<Pet> {
    COMPANION_PET_DATA
        .iter()
        .filter(|&&(g, _, _, _, _)| g == active_pet || owned_items.contains(&g))
        .filter_map(|&(g, _, _, _, _)| build_pet_for_companion(g, owned_items))
        .map(|mut p| {
            p.unknown2 = true;
            if let Some(&att) = equipped.get(&p.pet_id) {
                p.item_guid.push(ItemGuid { guid: att });
            }
            p
        })
        .collect()
}

/// Meshes that attachments add to the companion model:
/// (attachment item_guid, Cust_*.adr, texture alias).
///
/// Texture aliases come from the material names inside each Cust_*.adr (they
/// match the `Icon_Item_<adr>_<alias>_64.dds` icon names).  Attachments not
/// listed here (datacards, language packs, ...) add no mesh.
pub static COMPANION_ATTACHMENT_MODELS: &[(u32, &str, &str)] = &[
    // Mini AT-AT
    (563,  "Cust_ATAT_Spinners.adr",                    ""),            // Spinners
    (2713, "Cust_ATAT_Puppy.adr",                       ""),            // Puppy Datacard
    // Astromechs
    (309,  "Cust_AstroMechR2_Rockets.adr",              "Teal"),        // Thruster Jets
    (427,  "Cust_AstroMech_PogoLeg.adr",                "PogoLeg"),     // Pogo Leg
    (438,  "Cust_AstroMech_ElectricProdArm.adr",        "ElectroProdArm"), // Prod Arm
    (2714, "Cust_AstroMech_SmokeGenerator.adr",         "Default1"),    // Smokescreen Generator
    (2205, "Cust_AstroMech_SnowBlowerHead.adr",         "Default0"),    // Snow Maker
    // LEP servant droids
    (445,  "Cust_LEPServantDroid_HoloProjector.adr",    "HoloProjector"), // Hologram Projector
    (446,  "Cust_LEPServantDroid_Sumo.adr",             "Sumo"),        // Sparring Datacard
    (558,  "Cust_AstroMech_BlastromechHead.adr",        "BlastromechHead"), // Blastromech Gear
    (2155, "Cust_AstroMech_LifeDayLights.adr",          "Blinking"),    // Astromech Light Kit
    (2085, "Cust_AstroMech_Ghost.adr",                  "GhostEyes"),   // Eyes Sheet
    (2087, "Cust_AstroMech_Ghost.adr",                  "GhostC3PO"),   // 3PO Sheet
    (2088, "Cust_AstroMech_Ghost.adr",                  "GhostCloneTrooper"), // Clone Helmet Sheet
    (2089, "Cust_AstroMech_Ghost.adr",                  "GhostCyclops"), // Cyclops Sheet
    // Protocol droids
    (311,  "Cust_ProtocolDroid3PO_DrinkTray.adr",       "TealCarafe"),  // Serving Tray
    (425,  "Cust_ProtocolDroid_HeavyBlasterArms.adr",   "Gunmetal"),    // Heavy Blaster Arms
    (2109, "Cust_ProtocolDroid_GrievousCostume.adr",    ""),            // Grievous Costume
    (2156, "Cust_ProtocolDroid_LifeDayLights.adr",      "Blinking"),    // Protocol Droid Light Kit
    (2086, "Cust_ProtocolDroid_DraculaCape.adr",        ""),            // Count Dooku-la Cape
    // Mouse droids
    (424,  "Cust_MouseDroid_Rockets.adr",               "RedStripe"),   // Rocket Boosters
    (444,  "Cust_MouseDroid_CloneAntenna.adr",          "CloneBasic"),  // Bobblehead
    (2157, "Cust_MouseDroid_LifeDayLights.adr",         "Blinking"),    // Mouse Droid Light Kit
    (2158, "Cust_MouseDroid_WindUpKey.adr",             "WindUpKey"),   // Wind-up Key
    (2819, "Cust_MouseDroid_TopHat.adr",                ""),            // Mouse Droid Top Hat
    (2381, "Cust_MouseDroid_HoloProjector.adr",         ""),            // Candy Dispenser (holo hearts) — guess
    // Power droids
    (428,  "Cust_PowerDroid_PowerGenerator.adr",        "YellowStripe"), // Power Generator
    (429,  "Cust_PowerDroid_LobotBoombox.adr",          "YellowStripe"), // Boombox
    (436,  "Cust_PowerDroid_BoxingGlove.adr",           "Default0"),    // Boxing Gloves
    (2382, "Cust_PowerDroid_CupidGenerator.adr",        ""),            // Potion Pack — guess
    // Techno-service droid
    (562,  "Cust_TechnoServiceDroid_Bomb.adr",          "RedGlow"),     // Explosive Device
    (561,  "Cust_TechnoServiceDroid_NutsBolts.adr",     "Grey"),        // Wheel-Mode Datacard: the nuts & bolts it juggles
    // Creatures
    (2715, "Cust_Convor_JetPack.adr",                   "JetPack"),     // Convor Jetpack Gear
    (2716, "Cust_Convor_Pilot.adr",                     "Pilot"),       // Convor Pilot Gear
    (3288, "Cust_KowakianMonkeyLizard_FezCymbols.adr",  "FezCymbols"),  // Fez & Cymbals (Trained Kowakian gear)
    (3274, "Cust_KowakianMonkeyLizard_RobesLightsaber.adr", "SithRed"), // Kowakian Sith Robe
    (3275, "Cust_KowakianMonkeyLizard_RobesLightsaber.adr", "JediBlue"), // Kowakian Jedi Robe
    (3285, "Cust_Sarlacc_Balloon.adr",                  "Balloon"),     // Sarlacc Balloon
    (3289, "Cust_Sarlacc_MonCalamariHead.adr",          "MonCalamariHead"), // Mon Calamari Hat
];

/// Attachments that switch the companion to an alternate locomotion set:
/// (attachment item_guid, N) → AnimationTypes.xml slots Loc_AttachN_Stand /
/// _Walk / _Run = ids 581+20N / 582+20N / 583+20N (601/602/603 for N=1).
/// N values come from the Loc_AttachN entries in each companion's Char_*.adr
/// and the matching Cust_*.adr.
pub static COMPANION_ATTACHMENT_ANIM_SETS: &[(u32, i32)] = &[
    // Mini AT-AT (Char_ATAT.adr)
    (564,  1), // Bipedal Datacard
    (563,  2), // Spinners
    (2713, 3), // Puppy Datacard
    // Astromechs (Char_AstroMechR2/R4/R5.adr)
    (309,  1), // Thruster Jets → hover
    (427,  2), // Pogo Leg
    (438,  3), // Prod Arm
    (2714, 6), // Smokescreen Generator
    (558,  4), // Blastromech Gear → three-legged stance (StandThreeLegs)
    // LEP servant droids (Char_LEPServantDroid.adr)
    (446,  1), // Sparring Datacard → sparring stance
    // Protocol droids (Char_ProtocolDroid3PO.adr / Char_DeathStarDroidR7.adr)
    (311,  1), // Serving Tray
    (425,  2), // Heavy Blaster Arms
    (430,  3), // Oil Slick Footpads
    (447,  4), // Dance Groove Datacard
    (2109, 6), // Grievous Costume
    (559,  5), // Tusken Language Pack (normal stance)
    // Mouse droids (Char_MouseDroid.adr)
    (424,  1), // Rocket Boosters
    (444,  2), // Bobblehead
    (2158, 3), // Wind-up Key (not on 1M-AU5, see below)
    (2819, 4), // Top Hat
    // Power droid (Char_PowerDroid.adr)
    (436,  2), // Boxing Gloves
    // Probe droid (Char_ProbeDroid.adr)
    (658,  1), // Jurassic Datacard
    (659,  2), // Missile Launcher
    (657,  3), // UFO Datacard (saucer)
    // Techno-service droid (Char_TechnoServiceDroid.adr)
    (561,  2), // Wheel-Mode Datacard
    (562,  1), // Explosive Device (normal stance; explode one-shot)
    (560,  3), // Repulsor-Lifts → hover
    // Creatures
    (2715, 1), // Convor Jetpack (stand only)
    (2716, 2), // Convor Pilot (stand only)
    (3288, 1), // Fez & Cymbals
    (3275, 4), // Kowakian Jedi Robe
    (3274, 5), // Kowakian Sith Robe
    (3285, 1), // Sarlacc Balloon
    (3289, 2), // Mon Calamari Hat (Char_Sarlacc Loc_Attach2)
];

/// (pet_id, N) pairs whose model lacks the Loc_AttachN set (fall back to the
/// base locomotion).  Char_MouseDroidGold.adr has no Loc_Attach3.
static COMPANION_MISSING_ANIM_SETS: &[(u32, i32)] = &[(2092, 3)];

/// Companions whose Loc_AttachN sets only define _Stand (walk/run fall back to
/// loc_walk / loc_run).  Char_Convor.adr.
static COMPANION_STAND_ONLY_ANIM_SETS: &[u32] = &[2752, 3219, 3470, 3471, 3472];

/// (stand, walk, run) animation ids for companion `pet_id` with `equipped`
/// attached.  Defaults are loc_stand / loc_walk / loc_run (1 / 2 / 3).
pub fn companion_anim_ids(pet_id: u32, equipped: Option<u32>) -> (i32, i32, i32) {
    equipped
        .and_then(|att| COMPANION_ATTACHMENT_ANIM_SETS.iter().find(|(g, _)| *g == att))
        .filter(|&&(_, n)| !COMPANION_MISSING_ANIM_SETS.contains(&(pet_id, n)))
        .map(|&(_, n)| {
            let base = 581 + 20 * n;
            if COMPANION_STAND_ONLY_ANIM_SETS.contains(&pet_id) {
                (base, 2, 3)
            } else {
                (base, base + 1, base + 2)
            }
        })
        .unwrap_or((1, 2, 3))
}

/// Extra meshes for attachments made of more than one part, attached in a
/// second equipment slot so they don't replace the main mesh:
/// (attachment item_guid, Cust/other .adr, texture alias).
pub static COMPANION_ATTACHMENT_EXTRA_MODELS: &[(u32, &str, &str)] = &[
    // (none yet)
];

/// Composite effects applied to the companion itself while an attachment is
/// equipped (Attachment.composite_effect on the main mesh).  Effects that use
/// MESH parts bound to a bone need the companion's skeleton — attached to a
/// skeleton-less Cust_*.adr only their particles render.
pub static COMPANION_ATTACHMENT_EFFECTS: &[(u32, u32)] = &[
    // (none needed yet)
    // The Candy Dispenser's PFX_Pets_MouseDroid_HoloHeart (2337: red beam, hologram
    // heart, heart particles) is built into Cust_MouseDroid_HoloProjector.adr itself,
    // so adding it here drew a second heart.  Its mesh parts only render when the
    // client can load ActorMeshEmitterDefinitions.xml (Assets_006.pack).
];

/// Composite-effect tag id used for a companion's attachment effect.  Tags are
/// per character, so this only has to be unique among the companion's own tags.
const COMPANION_EFFECT_TAG: u32 = 1;

fn companion_attachment_effect(attachment: Option<u32>) -> Option<u32> {
    let att = attachment?;
    COMPANION_ATTACHMENT_EFFECTS.iter().find(|(g, _)| *g == att).map(|&(_, e)| e)
}

/// Packets that (re)apply the persistent attachment effect on a companion:
/// always clears the tag, then adds the new attachment's effect if it has one.
/// (Attachment.composite_effect alone only renders the effect's particles.)
fn companion_effect_tag_packets(companion_guid: u64, attachment: Option<u32>) -> Vec<Vec<u8>> {
    let mut packets = vec![GamePacket::serialize(&TunneledPacket {
        unknown1: true,
        inner: RemoveCompositeEffectTag { guid: companion_guid, tag_id: COMPANION_EFFECT_TAG },
    })];
    if let Some(effect) = companion_attachment_effect(attachment) {
        packets.push(GamePacket::serialize(&TunneledPacket {
            unknown1: true,
            inner: AddCompositeEffectTag {
                guid: companion_guid,
                tag_id: COMPANION_EFFECT_TAG,
                composite_effect_id: effect,
                triggered_by_guid: 0,
                unknown2: 0,
            },
        }));
    }
    packets
}

/// Composite effects played on the companion when it is clicked.  Each entry is
/// a list of options (one picked at random per click, like the one-shot tricks);
/// each option is a sequence of (effect id, delay ms, duration ms) played together.
pub static COMPANION_CLICK_EFFECTS: &[(u32, &[&[(u32, u32, u32)]])] = &[
    // Candy Dispenser: PFX_Pets_MouseDroid_Candy_Land / _Candy_P2P
    (2381, &[&[(1686, 0, 5500)], &[(1687, 0, 3000)]]),
    // Power Generator: PFX_Pets_PowerDroid_Electricity_one-shot / _explosion
    (428,  &[&[(1976, 0, 3000)], &[(1977, 0, 3000)]]),
    // Potion Pack: PFX_Pets_PowerDroid_Love_Pedal_Explode / _Love_Potion_skel / _Love_CoolDown
    (2382, &[&[(2354, 0, 3000)], &[(2356, 0, 3000)], &[(2355, 0, 3000)]]),
    // Snow Maker: PFX_Pets_Astromech_Snowblower_Explosion (8 s) / _Snowblower_Freeze (6 s)
    (2205, &[&[(2297, 0, 8000)], &[(2298, 0, 6000)]]),
    // Missile Launcher (probe droid): PFX_Pets_ProbeDroid_Missiles_Launch
    (659,  &[&[(1797, 0, 4000)]]),
    // Hologram Projector (LEP / "RabbitDroid"): PFX_Pets_RabbitDroid_Hologram_* — a random hologram
    (445,  &[&[(1519, 0, 5000)], &[(1560, 0, 5000)], &[(1561, 0, 5000)], &[(1562, 0, 5000)], &[(1563, 0, 5000)],
             &[(1564, 0, 5000)], &[(1565, 0, 5000)], &[(1566, 0, 5000)], &[(1567, 0, 5000)], &[(1568, 0, 5000)]]),
    // Prod Arm: one zap = PFX_Pets_Astromech_Prod_launch → _Prod_persist → _Prod_land
    (438,  &[&[(1523, 0, 1000), (1508, 300, 1500), (1509, 1200, 1500)]]),
    // Party Datacards: PFX_Pets_Astromech_Party (+ DancePack2 2.0 / 3.0 / 4.0)
    (310,  &[&[(1520, 0, 12000)]]),
    (2903, &[&[(2526, 0, 14000)]]),
    (2904, &[&[(2527, 0, 16000)]]),
    (2905, &[&[(2528, 0, 23000)]]),
];

/// Fixed animations played on click, for attachments whose trick isn't a
/// Loc_AttachN one-shot: (attachment, companion animation, player animation or
/// 0 for none, duration s).  The player animation plays on whoever clicked.
/// Broadcasts to send once a timed companion effect ends (e.g. restoring the
/// stand animations after a Party Datacard dance).  Drained once a second by
/// GameServer::tick_matchmaking_groups → due_companion_restores().
static PENDING_COMPANION_RESTORES: std::sync::Mutex<Vec<(std::time::Instant, Broadcast)>> =
    std::sync::Mutex::new(Vec::new());

fn schedule_companion_restore(after: std::time::Duration, broadcast: Broadcast) {
    if let Ok(mut pending) = PENDING_COMPANION_RESTORES.lock() {
        pending.push((std::time::Instant::now() + after, broadcast));
    }
}

/// Returns (and removes) the restore broadcasts whose time has come.
pub fn due_companion_restores() -> Vec<Broadcast> {
    let now = std::time::Instant::now();
    let Ok(mut pending) = PENDING_COMPANION_RESTORES.lock() else { return vec![] };
    let mut due = vec![];
    let mut i = 0;
    while i < pending.len() {
        if pending[i].0 <= now {
            due.push(pending.swap_remove(i).1);
        } else {
            i += 1;
        }
    }
    due
}

pub static COMPANION_CLICK_ANIMATIONS: &[(u32, i32, i32, f32)] = &[
    // Party Datacards: astromech dances (Emo_Dance_Basic1) and so does the player.
    // The Dance Pack II moves (3106-3108) are 14 / 16 / 23 s routines that play to
    // the end regardless, so the music and droid dance are stretched to match them.
    (310,  3101, 3101, 12.0), // music ~12 s
    (2903, 3101, 3106, 14.0), // Dance Pack II move 1 is a 14 s routine; music + droid match it
    (2904, 3101, 3107, 16.0), // Dance Pack II move 2 is a 16 s routine; music + droid match it
    (2905, 3101, 3108, 23.0), // Dance Pack II move 3 is a 23 s routine; music + droid match it
];

/// Extra meshes that change each time the companion is clicked, cycling in order
/// (index 0 is what COMPANION_ATTACHMENT_EXTRA_MODELS spawns with).
pub static COMPANION_CLICK_MESH_CYCLES: &[(u32, &[(&str, &str)])] = &[
    // (none yet — the coloured MouseDroid_Heart*.adr models turned out to be candy
    // pieces, not the Candy Dispenser's hologram heart)
];

/// Equipment slots used on the companion NPC: main attachment mesh, extra mesh.
const COMPANION_MAIN_MESH_SLOT: EquipmentSlot = EquipmentSlot::Body;
const COMPANION_EXTRA_MESH_SLOT: EquipmentSlot = EquipmentSlot::Head;

fn mesh(model: &str, texture: &str, slot: EquipmentSlot) -> Attachment {
    Attachment {
        model_name: model.to_string(),
        texture_alias: texture.to_string(),
        tint_alias: String::new(),
        tint: 0,
        composite_effect: 0,
        slot,
    }
}

/// The meshes for a companion with `equipped` attached: the main mesh (Body slot)
/// and, for multi-part attachments, the extra mesh (Head slot).
pub fn companion_npc_attachments(equipped: Option<u32>) -> Vec<Attachment> {
    let Some(att) = equipped else { return vec![] };
    let mut out = vec![];
    let effect = COMPANION_ATTACHMENT_EFFECTS.iter().find(|(g, _)| *g == att).map(|&(_, e)| e);
    if let Some(&(_, model, texture)) = COMPANION_ATTACHMENT_MODELS.iter().find(|(g, _, _)| *g == att) {
        let mut m = mesh(model, texture, COMPANION_MAIN_MESH_SLOT);
        m.composite_effect = effect.unwrap_or(0);
        out.push(m);
    }
    if let Some(&(_, model, texture)) = COMPANION_ATTACHMENT_EXTRA_MODELS.iter().find(|(g, _, _)| *g == att) {
        out.push(mesh(model, texture, COMPANION_EXTRA_MESH_SLOT));
    }
    out
}

/// Returns a single packet that shows `InactivePetWindow` directly, bypassing
/// `PetController.show()` (which would trigger the FTE tutorial on first login).
/// Send this at `ClientIsReady` so the pet panel is always visible at spawn.
pub fn show_inactive_pet_window() -> Vec<Vec<u8>> {
    vec![GamePacket::serialize(&TunneledPacket {
        unknown1: true,
        inner: ExecuteScriptWithIntParams {
            script_name: "InactivePetWindow.Show".to_string(),
            params: vec![],
        },
    })]
}

/// Returns the packets needed to show/hide the companion HUD for the player.
///
/// `pet_id != 0` bypasses `PetController.SetActivePetId` (which has an FTE guard
/// that blocks `ActivePetWindow`) and instead calls the steps that
/// `PetsHandler_PetIsActiveUpdate` performs natively:
///   1. Hide `InactivePetWindow` directly (keeps `PetController.isShown` intact)
///   2. Set the pet ID on `ActivePetWindow`
///   3. Set the attachment item group (enables/disables the gear icon)
///   4. Show `ActivePetWindow`
///
/// `pet_id == 0` reverses the process: hides `ActivePetWindow` and shows
/// `InactivePetWindow` directly (bypassing FTE).
pub fn set_active_pet_id(
    pet_id: u32,
    companion_guid: u64,
    owned_items: &std::collections::HashSet<u32>,
    equipped: &HashMap<u32, u32>,
) -> Vec<Vec<u8>> {
    if pet_id != 0 {
        // Look up attachment item group for gear icon — 0 means no gear (grayed out).
        let attachment_group = COMPANION_ATTACHMENT_GROUPS
            .iter()
            .find(|(guid, _)| *guid == pet_id)
            .map(|(_, g)| *g)
            .unwrap_or(0);

        // Steps 1–2: register the active pet before doing anything else.
        let mut packets = vec![
            // Step 1: hide the "no pet" panel.  Close InactivePetWindow directly —
            // NOT PetController.hide, which clears PetController.isShown.  The HUD
            // relies on that flag: opening the store / Play Games calls
            // PetController:hide() (closes both pet windows only if isShown) and
            // closing it calls PetController:show() → PetsHandler:PetIsActive(id),
            // which re-opens ActivePetWindow.  With isShown cleared the window was
            // left "open" behind the full-screen UI and never re-shown.
            GamePacket::serialize(&TunneledPacket {
                unknown1: true,
                inner: ExecuteScriptWithIntParams {
                    script_name: "InactivePetWindow.Hide".to_string(),
                    params: vec![],
                },
            }),
            // Step 2a: register the active pet in the C++ PetController.
            GamePacket::serialize(&TunneledPacket {
                unknown1: true,
                inner: ExecuteScriptWithIntParams {
                    script_name: "PetController.SetActivePetId".to_string(),
                    params: vec![pet_id as i32],
                },
            }),
            // Step 2b: also tell ActivePetWindow directly (sets self.petId for the
            // window's own logic, independent of PetController).
            GamePacket::serialize(&TunneledPacket {
                unknown1: true,
                inner: ExecuteScriptWithIntParams {
                    script_name: "ActivePetWindow.SetActivePetId".to_string(),
                    params: vec![pet_id as i32],
                },
            }),
        ];

        // Step 3 (conditional): set gear group ID so the gear icon is enabled.
        // ScriptsBase.bin: Lua calls ActivePetWindow.SetPetAttachmentItemGroupId(id) directly
        // on the window root (capital S, no mcActivePetView sub-path).
        if attachment_group != 0 {
            crate::info!(
                "SetPetAttachmentItemGroupId pet_id={pet_id} group={attachment_group}"
            );
            packets.push(GamePacket::serialize(&TunneledPacket {
                unknown1: true,
                inner: ExecuteScriptWithIntParams {
                    script_name: "ActivePetWindow.SetPetAttachmentItemGroupId".to_string(),
                    params: vec![attachment_group as i32],
                },
            }));
        } else {
            crate::info!(
                "SetPetAttachmentItemGroupId pet_id={pet_id}: no group mapping, gear icon stays grayed out"
            );
        }

        // Step 4: load the pet records and mark this pet active (PetInventory).
        packets.extend(build_pet_inventory_packets(pet_id, companion_guid, owned_items, equipped));

        // Step 5: show ActivePetWindow.
        packets.push(GamePacket::serialize(&TunneledPacket {
            unknown1: true,
            inner: ExecuteScriptWithIntParams {
                script_name: "ActivePetWindow.Show".to_string(),
                params: vec![],
            },
        }));

        packets
    } else {
        // Clear the client's active pet (pet_controller[0x84] / companion guid).
        // Without this Pet.GetActivePetId() keeps returning the dismissed pet and
        // the PetList summon handler bails with "Pet[ id ] is already summoned."
        let mut packets = build_pet_inventory_packets(0, 0, owned_items, equipped);
        packets.extend(vec![
            // Hide ActivePetWindow and restore InactivePetWindow directly
            // (bypasses PetController.show() which would trigger the FTE tutorial).
            GamePacket::serialize(&TunneledPacket {
                unknown1: true,
                inner: ExecuteScriptWithIntParams {
                    script_name: "ActivePetWindow.Hide".to_string(),
                    params: vec![],
                },
            }),
            GamePacket::serialize(&TunneledPacket {
                unknown1: true,
                inner: ExecuteScriptWithIntParams {
                    script_name: "InactivePetWindow.Show".to_string(),
                    params: vec![],
                },
            }),
        ]);
        packets
    }
}

/// Builds the `PetInventory` (Pet sub-op 0x05) packet that loads the player's
/// pet records into the client and marks `pet_id` as the active pet.
///
/// The client's attachment flyout (`BaseClient.PetInventory`) is not filled from
/// this packet directly: it lists inventory items with Item Type 3 whose
/// `item_class` is a key in the active pet record's `battle_class_items`.  This
/// packet provides those records and the active pet id.
pub fn build_pet_inventory_packets(
    pet_id: u32,
    companion_guid: u64,
    owned_items: &HashSet<u32>,
    equipped: &HashMap<u32, u32>,
) -> Vec<Vec<u8>> {
    let pets = owned_pet_records(pet_id, owned_items, equipped);
    crate::info!(
        "PetInventory: active_pet_id={pet_id} companion_guid={companion_guid:#018x} ({} pet records)",
        pets.len()
    );
    vec![GamePacket::serialize(&TunneledPacket {
        unknown1: true,
        inner: PetInventory {
            pets,
            active_pet_id: pet_id,
            companion_guid,
        },
    })]
}

/// Sends `ActivePetWindow.SetPetAttachmentItemGroupId` (enables the gear icon).
///
/// Called from `ClickSummonPetButton`, which arrives after the ActivePetWindow
/// SWF has loaded.  Note the client also sets this itself from the
/// `BaseClient.PetList` "Attachment ItemGroup ID" column (Pet.unknown11) when it
/// processes PetInventory.
pub fn send_attachment_group_id(pet_id: u32) -> Vec<Vec<u8>> {
    let attachment_group = COMPANION_ATTACHMENT_GROUPS
        .iter()
        .find(|(guid, _)| *guid == pet_id)
        .map(|(_, g)| *g)
        .unwrap_or(0);

    let mut packets = Vec::new();
    if attachment_group != 0 {
        packets.push(GamePacket::serialize(&TunneledPacket {
            unknown1: true,
            inner: ExecuteScriptWithIntParams {
                script_name: "ActivePetWindow.SetPetAttachmentItemGroupId".to_string(),
                params: vec![attachment_group as i32],
            },
        }));
    }
    packets
}

/// Re-sends the gear group id for `pet_id`.
///
/// Called from flyout-related UI events (onAttachmentsButtonClick etc.).  The
/// flyout contents come from the PetInventory sent at summon / equip time.
pub fn resend_pet_inventory(
    pet_id: u32,
    companion_guid: u64,
    owned_items: &std::collections::HashSet<u32>,
) -> Vec<Vec<u8>> {
    let _ = (companion_guid, owned_items);
    send_attachment_group_id(pet_id)
}

pub fn spawn_companion_npc(
    companion_guid: u64,
    model_id: u32,
    name_id: u32,
    spawn_pos: Pos,
    spawn_rot: Pos,
    attachment_group: u32,
    attachments: Vec<Attachment>,
    (stand_anim, walk_anim, run_anim): (i32, i32, i32),
) -> Vec<Vec<u8>> {
    vec![GamePacket::serialize(&TunneledPacket {
        unknown1: true,
        inner: AddNpc {
            guid: companion_guid,
            name_id,
            model_id,
            unknown3: false,
            chat_text_color: Character::DEFAULT_CHAT_TEXT_COLOR,
            chat_bubble_color: Character::DEFAULT_CHAT_BUBBLE_COLOR,
            chat_scale: 1,
            scale: 1.0,
            pos: spawn_pos,
            rot: spawn_rot,
            spawn_animation_id: 0,
            attachments,
            hostility: Hostility::Neutral,
            unknown10: 0,
            texture_alias: "".to_string(),
            tint_name: "".to_string(),
            tint_id: 0,
            unknown11: true,
            offset_y: 0.0,
            composite_effect_id: 0,
            wield_type: WieldType::None,
            name_override: "".to_string(),
            hide_name: false,
            name_offset_x: 0.0,
            name_offset_y: 0.0,
            name_offset_z: 0.0,
            terrain_object_id: 0,
            enable_attachments: true,
            speed: 8.0,
            unknown21: false,
            interactable_size_pct: 100,
            walk_animation_id: walk_anim,
            sprint_animation_id: run_anim,
            stand_animation_id: stand_anim,
            unknown26: false,
            disable_gravity: false,
            sub_title_id: 0,
            one_shot_animation_id: -1,
            temporary_model: 0,
            effects: vec![],
            disable_interact_popup: true,
            unused_death_animation_id: 0,
            unknown34: false,
            show_health: false,
            hide_despawn_fade: false,
            enable_tilt: false,
            base_attachment_group: BaseAttachmentGroup {
                unknown1: attachment_group,
                unknown2: "".to_string(),
                unknown3: "".to_string(),
                unknown4: 0,
                unknown5: "".to_string(),
            },
            tilt: Pos {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                w: 0.0,
            },
            unknown40: 0,
            bounce_area_id: -1,
            image_set_id: 0,
            // Clickable so clicking the companion sends InteractRequest → attachment trick.
            clickable: true,
            rider_guid: 0,
            physics: PhysicsState::default(),
            interact_popup_radius: 0.0,
            target: Target::default(),
            variables: vec![],
            rail_id: 0,
            rail_elapsed_seconds: 0.0,
            rail_offset: Pos {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                w: 0.0,
            },
            unknown54: 0,
            rail_unknown1: 0.0,
            rail_unknown2: 0.0,
            auto_interact_radius: 0.0,
            head_customization_override: "".to_string(),
            hair_customization_override: "".to_string(),
            body_customization_override: "".to_string(),
            override_terrain_model: false,
            hover_glow: 0,
            hover_description: 0,
            fly_over_effect: 0,
            unknown65: 0,
            unknown66: 0,
            unknown67: 0,
            disable_move_to_interact: true,
            unknown69: 0.0,
            unknown70: 0.0,
            unknown71: 0,
            icon_id: Icon::None,
        },
    }),
    // Give the companion an interact cursor.  Like regular NPCs (BaseNpc::add_packets),
    // an NPC only becomes clickable — and sends InteractRequest — once NpcRelevance
    // assigns it a cursor.  54 is the cursor most configured NPCs use.
    GamePacket::serialize(&TunneledPacket {
        unknown1: true,
        inner: NpcRelevance {
            new_states: vec![SingleNpcRelevance {
                guid: companion_guid,
                cursor: Some(COMPANION_CURSOR),
                unknown1: false,
            }],
        },
    })]
}

/// Mouse cursor shown when hovering a companion (see NpcRelevance above).
const COMPANION_CURSOR: u8 = 54;

pub fn process_companion_packet(
    cursor: &mut Cursor<&[u8]>,
    sender: u32,
    game_server: &GameServer,
) -> Result<Vec<Broadcast>, ProcessPacketError> {
    // Packet format (6 bytes):
    //   [04]  [<item_guid: u32 LE>]  [01]
    //    cmd   companion item_guid   summon flag (1=summon, 0=unsummon, unconfirmed)
    //
    // cmd=0x00 appears to be a side-effect packet from ClickUnEquipAttachmentButton
    // (bytes 0x00 01 01 00 00, produces item_guid=257=0x00000101).  Ignore it.
    // cmd=0x04 is the summon/dismiss packet.
    // Log all raw bytes for debugging unhandled Pet sub-commands.
    let start = cursor.position() as usize;
    let raw = &cursor.get_ref()[start..];
    crate::info!("Pet packet from {sender}: raw={:02x?}", &raw[..raw.len().min(16)]);

    let cmd: u8 = DeserializePacket::deserialize(cursor).unwrap_or(0);
    let item_guid: u32 = DeserializePacket::deserialize(cursor).unwrap_or(0);
    let _summon: u8 = DeserializePacket::deserialize(cursor).unwrap_or(0);

    crate::info!("Companion packet from {sender}: cmd={cmd:#04x} item_guid={item_guid}");

    // Known cmd values (from packet captures):
    //   0x00 → ClickUnEquipAttachmentButton side-effect; item_guid=257; ignore.
    //   0x04 → Summon / dismiss companion; item_guid = companion item guid.
    //
    // Unknown cmds (to be decoded from a live pcap):
    //   0x01/0x02/0x03/0x05 — may include equip-attachment; log and skip for now.
    //
    // TODO: once a pcap of clicking an attachment in the pet-attachment flyout is
    // captured, decode the cmd here and implement equip-attachment handling:
    //   1. Store equipped attachment (item_guid) on the Player character.
    //   2. Determine the correct COMPANION_ANIMATION_GROUPS value for that item.
    //   3. Despawn + respawn the active companion with the new attachment_group.
    match cmd {
        0x00 => return Ok(vec![]),
        0x0b => {
            // Attachment equip/remove from the flyout: [0b][flag u8][slot u32][item u32]
            //   equip 564:  0b 00 01000000 34020000
            //   remove:     0b 01 01000000 00000000  (Lua Pet.UnequipItem(0, 1))
            let buf: &[u8] = *cursor.get_ref();
            let pkt = &buf[start..];
            let rd = |o: usize| pkt.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
            let flag = rd(2).unwrap_or(0);
            let attachment = rd(6).unwrap_or(0);
            crate::info!(
                "Companion attachment request from {sender}: flag={flag} item={attachment}"
            );
            return change_companion_attachment(
                sender,
                if attachment != 0 { Some(attachment) } else { None },
                game_server,
            );
        }
        0x04 => { /* fall through to summon/dismiss logic below */ }
        other => {
            crate::info!(
                "Unhandled companion cmd {other:#04x} from {sender} \
                 (item_guid={item_guid}) — likely attachment equip; needs pcap"
            );
            return Ok(vec![]);
        }
    }

    game_server
        .lock_enforcer()
        .read_characters(|_| CharacterLockRequest {
            read_guids: Vec::new(),
            write_guids: vec![player_guid(sender)],
            character_consumer: |characters_table_read_handle,
                                 _,
                                 mut characters_write,
                                 _minigame_data_lock_enforcer| {
                let Some(character) = characters_write.get_mut(&player_guid(sender)) else {
                    return Err(ProcessPacketError::new(
                        ProcessPacketErrorType::ConstraintViolated,
                        format!("Unknown player {sender} sent companion packet"),
                    ));
                };

                // Verify the requested companion belongs to this player and
                // snapshot purchased_items for filtering attachment items later.
                let (owns_companion, purchased_snapshot) = {
                    if let CharacterType::Player(player) = &character.stats.character_type {
                        (
                            player.companions.contains(&item_guid),
                            player.purchased_items.clone(),
                        )
                    } else {
                        return Err(ProcessPacketError::new(
                            ProcessPacketErrorType::ConstraintViolated,
                            format!("Non-player {sender} sent companion packet"),
                        ));
                    }
                };

                if !owns_companion {
                    crate::info!(
                        "Player {sender} requested companion {item_guid} they don't own; ignoring"
                    );
                    return Ok(vec![]);
                }

                // Snapshot mutable-access fields before calling into
                // characters_table_read_handle (which only needs a shared ref).
                let existing_guid = character.stats.companion_guid;
                let player_pos = character.stats.pos;
                let player_rot = character.stats.rot;
                let (_, instance_guid, chunk) = character.index1();

                let all_players_nearby = ZoneInstance::all_players_nearby(
                    chunk,
                    instance_guid,
                    characters_table_read_handle,
                );

                // Despawn the current companion if one is active.
                // If the player clicked the *same* companion that's already out, toggle it off.
                // If they clicked a *different* companion, despawn the old one and fall through
                // to spawn the new one in the same round-trip.
                let mut despawn_packet: Option<Vec<u8>> = None;
                if let Some(guid) = existing_guid {
                    let active_item = character.stats.active_companion_item_guid;
                    character.stats.companion_guid = None;
                    character.stats.active_companion_item_guid = None;
                    character.stats.companion_world_pos = None;
                    crate::info!(
                        "Despawning companion guid={guid:#018x} for player {sender}"
                    );
                    if active_item == Some(item_guid) {
                        // Same companion — toggle off with a graceful fade and return.
                        let remove_pkt = GamePacket::serialize(&TunneledPacket {
                            unknown1: true,
                            inner: RemoveGracefully {
                                guid,
                                use_death_animation: false,
                                delay_millis: 0,
                                composite_effect_delay_millis: 0,
                                composite_effect: 0,
                                fade_duration_millis: 1000,
                            },
                        });
                        return Ok(vec![
                            Broadcast::Multi(all_players_nearby, vec![remove_pkt]),
                            Broadcast::Single(
                                sender,
                                set_active_pet_id(0, 0, &purchased_snapshot, &character.stats.companion_attachments),
                            ),
                        ]);
                    }
                    // Different companion — instant despawn (fade_duration=0) so the same GUID
                    // can be reused for the new spawn without the client confusing the new
                    // AddNpc with the still-fading old entity.
                    let remove_pkt = GamePacket::serialize(&TunneledPacket {
                        unknown1: true,
                        inner: RemoveGracefully {
                            guid,
                            use_death_animation: false,
                            delay_millis: 0,
                            composite_effect_delay_millis: 0,
                            composite_effect: 0,
                            fade_duration_millis: 0,
                        },
                    });
                    despawn_packet = Some(remove_pkt);
                }

                // Look up model_id for this companion's item_guid.
                let Some(&model_id) = COMPANION_MODELS
                    .iter()
                    .find(|(guid, _)| *guid == item_guid)
                    .map(|(_, mid)| mid)
                else {
                    crate::info!(
                        "Player {sender} companion item_guid={item_guid} has no model mapping; ignoring"
                    );
                    return Ok(vec![]);
                };

                // Encode item_guid into bits 32-55 so each companion type gets a
                // unique NPC GUID.  This prevents the client confusing the new AddNpc
                // with the still-cached old entity when switching companions rapidly.
                // Bits 32-55 are always 0 in the base pet_guid (player GUID is u32).
                let companion_guid = pet_guid(player_guid(sender))
                    | (((item_guid as u64) & 0x00FF_FFFF) << 32);
                character.stats.companion_guid = Some(companion_guid);
                character.stats.active_companion_item_guid = Some(item_guid);
                character.stats.companion_click_count = 0;

                // Place companion 2.0 units behind the player so it doesn't clip.
                // rot.x is the heading (yaw) in radians; sin/cos give the forward vector.
                const COMPANION_OFFSET: f32 = 2.0;
                let heading = player_rot.x;
                let spawn_x = player_pos.x - heading.sin() * COMPANION_OFFSET;
                let spawn_y = player_pos.y;
                let spawn_z = player_pos.z - heading.cos() * COMPANION_OFFSET;
                let companion_spawn_pos = Pos {
                    x: spawn_x,
                    y: spawn_y,
                    z: spawn_z,
                    w: 1.0,
                };
                // Initialise tracked world position at the offset spawn point so
                // subsequent delta movement starts from the correct location.
                character.stats.companion_world_pos = Some((spawn_x, spawn_y, spawn_z));

                crate::info!(
                    "Spawning companion model_id={model_id} for player {sender} \
                     (item_guid={item_guid}, companion_guid={companion_guid:#018x}) \
                     at ({spawn_x:.2}, {spawn_y:.2}, {spawn_z:.2})"
                );

                // BaseAttachmentGroup.unknown1 in AddNpc controls which Loc_Attach*
                // animation variant the client activates.  Look up the default animation
                // group from COMPANION_ANIMATION_GROUPS (0 = base animations if not found).
                // Note: SetPetAttachmentItemGroupId (sent separately in set_active_pet_id)
                // uses the aggregate group from COMPANION_ATTACHMENT_GROUPS — independent.
                // TODO: once per-companion equipped-attachment tracking is implemented,
                // use the player's actual equipped attachment group here instead.
                let equipped_snapshot = character.stats.companion_attachments.clone();
                let spawn_animation_group = COMPANION_ANIMATION_GROUPS
                    .iter()
                    .find(|(guid, _)| *guid == item_guid)
                    .map(|(_, g)| *g)
                    .unwrap_or(0);
                let mut spawn_packets =
                    spawn_companion_npc(companion_guid, model_id, 0, companion_spawn_pos, player_rot, spawn_animation_group,
                        companion_npc_attachments(equipped_snapshot.get(&item_guid).copied()),
                        match equipped_snapshot.get(&item_guid).copied() {
                            Some(att) => companion_anim_ids(item_guid, Some(att)),
                            None => (1, -1, -1), // spawn defaults as before
                        });
                // Persistent attachment effect (e.g. Candy Dispenser hologram heart).
                if companion_attachment_effect(equipped_snapshot.get(&item_guid).copied()).is_some() {
                    spawn_packets.extend(companion_effect_tag_packets(
                        companion_guid,
                        equipped_snapshot.get(&item_guid).copied(),
                    ));
                }

                // Send despawn and spawn as SEPARATE broadcasts so the client has time
                // to evict the old entity from its table before AddNpc for the new one
                // arrives.  Combining them in one burst caused the new companion to never
                // appear when switching, even with fade_duration_millis=0.
                let mut broadcasts = Vec::new();
                if let Some(remove_pkt) = despawn_packet {
                    broadcasts.push(Broadcast::Multi(all_players_nearby.clone(), vec![remove_pkt]));
                }
                broadcasts.push(Broadcast::Multi(all_players_nearby, spawn_packets));
                // Show ActivePetWindow directly, bypassing PetController.SetActivePetId
                // (which has an FTE guard that blocks the window when isFTEShown=false).
                // We mirror what PetsHandler_PetIsActiveUpdate does natively:
                // hide InactivePetWindow, set pet ID, show ActivePetWindow.
                broadcasts.push(Broadcast::Single(sender, set_active_pet_id(item_guid, companion_guid, &purchased_snapshot, &equipped_snapshot)));
                Ok(broadcasts)
            },
        })
}

/// Equips (`Some`) or removes (`None`) an attachment on the player's active
/// companion.
///
/// Sends the owner a fresh PetInventory (the pet record's `item_guid` list is the
/// client's "equipped" list) plus a 0x0b echo (refreshes
/// `BaseClient.PetInventory.EquippedWearablesData`), and sends nearby players an
/// `UpdateEquippedItem` on the companion NPC with the attachment's mesh, if any.
pub fn change_companion_attachment(
    sender: u32,
    attachment: Option<u32>,
    game_server: &GameServer,
) -> Result<Vec<Broadcast>, ProcessPacketError> {
    game_server
        .lock_enforcer()
        .read_characters(|_| CharacterLockRequest {
            read_guids: Vec::new(),
            write_guids: vec![player_guid(sender)],
            character_consumer: |characters_table_read_handle, _, mut characters_write, _| {
                let Some(character) = characters_write.get_mut(&player_guid(sender)) else {
                    return Ok(vec![]);
                };
                let (Some(pet_id), Some(companion_guid)) = (
                    character.stats.active_companion_item_guid,
                    character.stats.companion_guid,
                ) else {
                    crate::info!("Attachment change from {sender} with no active companion; ignoring");
                    return Ok(vec![]);
                };

                if let Some(att) = attachment {
                    let allowed = COMPANION_PET_DATA
                        .iter()
                        .find(|&&(g, _, _, _, _)| g == pet_id)
                        .is_some_and(|&(_, _, _, _, atts)| atts.contains(&att));
                    if !allowed {
                        crate::info!("Player {sender}: attachment {att} not valid for companion {pet_id}; ignoring");
                        return Ok(vec![]);
                    }
                    character.stats.companion_attachments.insert(pet_id, att);
                } else {
                    character.stats.companion_attachments.remove(&pet_id);
                }
                crate::info!("Player {sender}: companion {pet_id} attachment -> {attachment:?}");
                character.stats.companion_click_count = 0; // click-cycled meshes restart

                let owned = if let CharacterType::Player(p) = &character.stats.character_type {
                    p.purchased_items.clone()
                } else {
                    Default::default()
                };
                let equipped = character.stats.companion_attachments.clone();
                let (_, instance_guid, chunk) = character.index1();
                let all_players_nearby =
                    ZoneInstance::all_players_nearby(chunk, instance_guid, characters_table_read_handle);

                let mut own = build_pet_inventory_packets(pet_id, companion_guid, &owned, &equipped);
                own.push(GamePacket::serialize(&TunneledPacket {
                    unknown1: true,
                    // Mirror the client's own request: flag 0 + item = equip,
                    // flag 1 + item 0 = remove (raw 0b 01 01000000 00000000).
                    inner: PetEquipAttachment {
                        flag: attachment.is_none(),
                        slot: 1,
                        item_id: attachment.unwrap_or(0),
                    },
                }));

                // Update both companion mesh slots every time so switching from a
                // two-part attachment to anything else clears the extra mesh.
                let meshes = companion_npc_attachments(attachment);
                let visuals: Vec<Vec<u8>> = [COMPANION_MAIN_MESH_SLOT, COMPANION_EXTRA_MESH_SLOT]
                    .into_iter()
                    .map(|slot| {
                        let m = meshes
                            .iter()
                            .find(|m| m.slot == slot)
                            .cloned()
                            .unwrap_or_else(|| mesh("", "", slot));
                        crate::info!(
                            "UpdateEquippedItem companion {companion_guid:#018x} slot {:?}: model={:?}",
                            slot as u32, m.model_name
                        );
                        GamePacket::serialize(&TunneledPacket {
                            unknown1: true,
                            inner: UpdateEquippedItem {
                                guid: companion_guid,
                                item_guid: attachment.unwrap_or(0),
                                item: m,
                                battle_class: 0,
                                wield_type: WieldType::None,
                            },
                        })
                    })
                    .collect();

                let (stand, walk, run) = companion_anim_ids(pet_id, attachment);
                crate::info!(
                    "Companion {companion_guid:#018x} animations: stand={stand} walk={walk} run={run}"
                );
                let anims = vec![
                    GamePacket::serialize(&TunneledPacket {
                        unknown1: true,
                        inner: UpdateIdleAnimation { guid: companion_guid, animation_id: stand },
                    }),
                    GamePacket::serialize(&TunneledPacket {
                        unknown1: true,
                        inner: UpdateWalkAnimation { guid: companion_guid, animation_id: walk },
                    }),
                    GamePacket::serialize(&TunneledPacket {
                        unknown1: true,
                        inner: UpdateRunAnimation { guid: companion_guid, animation_id: run },
                    }),
                ];
                let mut for_all = visuals;
                for_all.extend(companion_effect_tag_packets(companion_guid, attachment));
                for_all.extend(anims);

                Ok(vec![
                    Broadcast::Single(sender, own),
                    Broadcast::Multi(all_players_nearby, for_all),
                ])
            },
        })
}

/// Called on each player movement packet **before** `ZoneInstance::move_character`
/// updates `character.stats.pos`, so `character.stats.pos` still holds the player's
/// *previous* world position and we can compute an accurate delta.
///
/// The companion is moved by the same world-space delta as the player instead of
/// being pinned to a facing-relative "behind" offset.  This means:
/// - When the player walks the companion walks with them.
/// - When the player turns in place (delta ≈ 0) the companion stays put but
///   inherits the player's new rotation, so it turns without orbiting.
pub fn move_companion_if_active(
    sender: u32,
    pos_update: UpdatePlayerPos,
    game_server: &GameServer,
) -> Vec<Broadcast> {
    game_server
        .lock_enforcer()
        .read_characters(|_| CharacterLockRequest {
            read_guids: vec![],
            write_guids: vec![player_guid(sender)],
            character_consumer: |characters_table_read_handle,
                                  _,
                                  mut characters_write,
                                  _| {
                let Some(character) = characters_write.get_mut(&player_guid(sender)) else {
                    return Ok::<Vec<Broadcast>, ProcessPacketError>(vec![]);
                };

                let Some(companion_guid) = character.stats.companion_guid else {
                    // No companion out: if a full-screen window hid the "no pet"
                    // panel, the player moving means it has closed again.
                    if character.stats.refresh_inactive_pet_window {
                        character.stats.refresh_inactive_pet_window = false;
                        return Ok(vec![Broadcast::Single(sender, reopen_inactive_pet_window())]);
                    }
                    return Ok(vec![]);
                };

                let Some((_, instance_guid, chunk)) =
                    characters_table_read_handle.index1(player_guid(sender))
                else {
                    return Ok(vec![]);
                };

                // character.stats.pos is the player's position from the PREVIOUS tick
                // (move_companion_if_active is called before move_character updates it).
                let old_pos = character.stats.pos;
                let delta_x = pos_update.pos_x - old_pos.x;
                let delta_y = pos_update.pos_y - old_pos.y;
                let delta_z = pos_update.pos_z - old_pos.z;

                // Advance the companion's tracked world position by the same delta.
                let (cx, cy, cz) = character
                    .stats
                    .companion_world_pos
                    .unwrap_or((old_pos.x, old_pos.y, old_pos.z));
                let new_cx = cx + delta_x;
                let new_cy = cy + delta_y;
                let new_cz = cz + delta_z;
                character.stats.companion_world_pos = Some((new_cx, new_cy, new_cz));

                let all_players_nearby = ZoneInstance::all_players_nearby(
                    chunk,
                    instance_guid,
                    characters_table_read_handle,
                );

                // Build the companion's position update: same rotation as the player
                // (so it turns with them) but at the companion's tracked world pos.
                let mut companion_pos = pos_update;
                companion_pos.guid = companion_guid;
                companion_pos.pos_x = new_cx;
                companion_pos.pos_y = new_cy;
                companion_pos.pos_z = new_cz;
                // rot_x/y/z are inherited from pos_update → companion faces same direction.

                Ok(vec![Broadcast::Multi(
                    all_players_nearby,
                    vec![GamePacket::serialize(&TunneledPacket {
                        unknown1: true,
                        inner: companion_pos,
                    })],
                )])
            },
        })
        .unwrap_or_default()
}

/// One-shot "tricks" an attachment unlocks, played when the player clicks the
/// companion: (attachment item_guid, Loc_AttachN set, OneShot indices available).
/// Taken from the Loc_AttachN_OneShotK entries in each companion's Char_*.adr and
/// the attachment's Cust_*.adr.  Animation ids (AnimationTypes.xml):
///   OneShot1 = 584+20N, OneShot2 = 585+20N, OneShot3 = 590+20N.
pub static COMPANION_ATTACHMENT_TRICKS: &[(u32, i32, &[i32])] = &[
    // Mini AT-AT
    (564,  1, &[1]),        // Bipedal Datacard: dance
    (563,  2, &[1]),        // Spinners: hydraulic bounce
    (2713, 3, &[1, 2, 3]),  // Puppy Datacard: beg / chase / play dead
    // Astromechs
    (309,  1, &[1]),        // Thruster Jets
    (427,  2, &[1, 2, 3]),  // Pogo Leg: low / medium / high jump
    (2714, 6, &[1, 2]),     // Smokescreen Generator
    (558,  4, &[1, 2]),     // Blastromech Gear: short / long burst (mesh fires its own blaster FX)
    // LEP servant droids
    (446,  1, &[1, 2]),     // Sparring Datacard: headbutt / fall & get up
    // Protocol droids
    (425,  2, &[1, 2]),     // Heavy Blaster Arms
    (430,  3, &[1]),        // Oil Slick Footpads
    (447,  4, &[1]),        // Dance Groove Datacard
    (2109, 6, &[1]),        // Grievous Costume
    (559,  5, &[1]),        // Tusken Language Pack: Tusken speech (Datacard_Tusken)
    // Mouse droids
    (424,  1, &[1]),        // Rocket Boosters
    (444,  2, &[1, 2, 3]),  // Bobblehead
    (2158, 3, &[1]),        // Wind-up Key
    (2819, 4, &[1]),        // Top Hat: fireworks
    // Power droid
    (436,  2, &[1, 2]),     // Boxing Gloves
    // Probe droid
    (658,  1, &[1]),        // Jurassic Datacard: claw
    (657,  3, &[1, 2]),     // UFO Datacard: buzzsaw / shooter
    (429,  1, &[1]),        // Boombox: dance (Char_PowerDroid_Amb_Dance)
    // Techno-service droid
    (561,  2, &[1]),        // Wheel-Mode Datacard: juggle (NutsBolts mesh juggles along)
    (562,  1, &[1]),        // Explosive Device: explode (blink/explosion FX built into Char_TechnoServiceDroid.adr)
    (560,  3, &[1]),        // Repulsor-Lifts
    // Creatures
    (2715, 1, &[1, 2]),     // Convor Jetpack
    (2716, 2, &[1, 2]),     // Convor Pilot
    (3288, 1, &[1, 2]),     // Fez & Cymbals: cymbal smash
    (3275, 4, &[1, 2]),     // Kowakian Jedi Robe
    (3274, 5, &[1, 2]),     // Kowakian Sith Robe
    (3285, 1, &[1, 2]),     // Sarlacc Balloon: lift off / explode
    (3289, 2, &[1, 2]),     // Mon Calamari Hat: ink jet / ink explosion
];

/// If `target` is a spawned companion, plays one of its equipped attachment's
/// tricks for everyone nearby and returns Some(broadcasts).  Returns None when
/// `target` isn't a companion, so the caller can fall through to normal NPC
/// interaction.  Companion guids are pet_guid(owner) | (item_guid << 32).
pub fn companion_trick(requester: u64, target: u64, game_server: &GameServer) -> Option<Vec<Broadcast>> {
    if (target >> 56) as u8 != super::unique_guid::PET_DISCRIMINANT {
        return None;
    }
    let owner = (target & 0xFFFF_FFFF) as u32;
    let result = game_server
        .lock_enforcer()
        .read_characters(|_| CharacterLockRequest {
            read_guids: vec![],
            write_guids: vec![player_guid(owner)],
            character_consumer: |characters_table_read_handle, _, mut characters_write, _| {
                let Some(character) = characters_write.get_mut(&player_guid(owner)) else {
                    return Ok::<Vec<Broadcast>, ProcessPacketError>(vec![]);
                };
                if character.stats.companion_guid != Some(target) {
                    return Ok(vec![]);
                }
                let Some(pet_id) = character.stats.active_companion_item_guid else {
                    return Ok(vec![]);
                };
                let Some(attachment) = character.stats.companion_attachments.get(&pet_id).copied() else {
                    return Ok(vec![]);
                };
                let mut packets = vec![];

                // One-shot trick animation.
                if let Some(&(_, n, shots)) =
                    COMPANION_ATTACHMENT_TRICKS.iter().find(|(g, _, _)| *g == attachment)
                {
                    if !shots.is_empty() && !COMPANION_MISSING_ANIM_SETS.contains(&(pet_id, n)) {
                        // Random trick.  (Not the clock: Windows timestamps tick in
                        // 100ns steps, so subsec_nanos() % 2 was always 0.)
                        let k = *shots.choose(&mut rand::thread_rng()).unwrap_or(&1);
                        let animation_id = match k {
                            1 => 584 + 20 * n,
                            2 => 585 + 20 * n,
                            _ => 590 + 20 * n,
                        };
                        crate::info!(
                            "Companion {target:#018x} (pet {pet_id}, attachment {attachment}) trick: OneShot{k} → animation {animation_id}"
                        );
                        packets.push(GamePacket::serialize(&TunneledPacket {
                            unknown1: true,
                            inner: QueueAnimation {
                                character_guid: target,
                                animation_id,
                                queue_pos: 0,
                                delay_seconds: 0.0,
                                duration_seconds: 2.0,
                            },
                        }));
                    }
                }

                // Click composite effects (one option at random, each a timed sequence).
                if let Some(option) = COMPANION_CLICK_EFFECTS
                    .iter()
                    .find(|(g, _)| *g == attachment)
                    .and_then(|(_, options)| options.choose(&mut rand::thread_rng()))
                {
                    for &(effect, delay_millis, duration_millis) in option.iter() {
                        crate::info!(
                            "Companion {target:#018x} (attachment {attachment}) click effect {effect} (+{delay_millis} ms)"
                        );
                        packets.push(GamePacket::serialize(&TunneledPacket {
                            unknown1: true,
                            inner: PlayCompositeEffect {
                                guid: target,
                                triggered_by_guid: 0,
                                composite_effect: effect,
                                delay_millis,
                                duration_millis,
                                pos: Pos::default(),
                            },
                        }));
                    }
                }

                // Fixed click animations (e.g. Party Datacards: companion + player dance).
                if let Some(&(_, companion_anim, player_anim, duration_seconds)) =
                    COMPANION_CLICK_ANIMATIONS.iter().find(|(g, _, _, _)| *g == attachment)
                {
                    crate::info!(
                        "Companion {target:#018x} (attachment {attachment}) click animation {companion_anim}, player {requester:#x} animation {player_anim}"
                    );
                    let mut queue = vec![(target, companion_anim)];
                    if player_anim != 0 {
                        queue.push((requester, player_anim));
                    }
                    // Loop the dance for the full duration by making it the standing
                    // animation (one queued one-shot only plays ~8 s), then restore the
                    // normal stand animations from the timer queue.
                    let (_, instance_guid, chunk) = character.index1();
                    let nearby = ZoneInstance::all_players_nearby(chunk, instance_guid, characters_table_read_handle);
                    let (companion_stand, _, _) = companion_anim_ids(pet_id, Some(attachment));
                    packets.push(GamePacket::serialize(&TunneledPacket {
                        unknown1: true,
                        inner: UpdateIdleAnimation { guid: target, animation_id: companion_anim },
                    }));
                    let mut restore = vec![GamePacket::serialize(&TunneledPacket {
                        unknown1: true,
                        inner: UpdateIdleAnimation { guid: target, animation_id: companion_stand },
                    })];
                    for (guid, _) in queue.into_iter().skip(1) {
                        packets.push(GamePacket::serialize(&TunneledPacket {
                            unknown1: true,
                            inner: SetAnimation {
                                character_guid: guid,
                                animation_id: player_anim,
                                animation_group_id: -1,
                                override_animation: true,
                            },
                        }));
                        restore.push(GamePacket::serialize(&TunneledPacket {
                            unknown1: true,
                            // Must mirror how the dance was applied (override on);
                            // override_animation: false left the player dancing.
                            inner: SetAnimation {
                                character_guid: guid,
                                animation_id: 1, // loc_stand
                                animation_group_id: -1,
                                override_animation: true,
                            },
                        }));
                    }
                    schedule_companion_restore(
                        std::time::Duration::from_secs_f32(duration_seconds),
                        Broadcast::Multi(nearby, restore),
                    );
                }

                // Click-cycled extra mesh (none configured yet).
                if let Some(&(_, cycle)) = COMPANION_CLICK_MESH_CYCLES.iter().find(|(g, _)| *g == attachment) {
                    character.stats.companion_click_count = character.stats.companion_click_count.wrapping_add(1);
                    let (model, texture) = cycle[character.stats.companion_click_count as usize % cycle.len()];
                    crate::info!("Companion {target:#018x} (attachment {attachment}) mesh cycle → {model}");
                    packets.push(GamePacket::serialize(&TunneledPacket {
                        unknown1: true,
                        inner: UpdateEquippedItem {
                            guid: target,
                            item_guid: attachment,
                            item: mesh(model, texture, COMPANION_EXTRA_MESH_SLOT),
                            battle_class: 0,
                            wield_type: WieldType::None,
                        },
                    }));
                }

                if packets.is_empty() {
                    return Ok(vec![]);
                }
                let (_, instance_guid, chunk) = character.index1();
                let nearby = ZoneInstance::all_players_nearby(chunk, instance_guid, characters_table_read_handle);
                Ok(vec![Broadcast::Multi(nearby, packets)])
            },
        })
        .unwrap_or_default();
    Some(result)
}

/// UiInteractions that mean a full-screen window (store, Play Games) just opened.
///
/// Opening one hides the HUD; the client's PetController:hide() closes both pet
/// windows.  When it closes, PetController:show() only re-opens ActivePetWindow
/// (via PetsHandler:PetIsActive) — nothing client-side re-opens InactivePetWindow,
/// and the client sends no "closed" message.  Movement is disabled while these
/// windows are up, so the first movement update afterwards is our cue.
const FULLSCREEN_WINDOW_OPENED: &[(&str, &str)] = &[
    ("StoreWindow", "show"),
    ("Hud_Dock", "ClickStoreButton"),
    ("Hud_Dock", "ClickActivitiesButton"),
];

pub fn is_fullscreen_window_opened(window: &str, button: &str) -> bool {
    FULLSCREEN_WINDOW_OPENED.iter().any(|&(w, b)| w == window && b == button)
}

/// Flags the player so their next movement re-opens InactivePetWindow, if no
/// companion is out.  Called when a full-screen window opens.
pub fn note_fullscreen_window_opened(sender: u32, game_server: &GameServer) {
    let _ = game_server.lock_enforcer().read_characters(|_| CharacterLockRequest {
        read_guids: vec![],
        write_guids: vec![player_guid(sender)],
        character_consumer: |_, _, mut characters_write, _| {
            if let Some(character) = characters_write.get_mut(&player_guid(sender)) {
                if character.stats.companion_guid.is_none() {
                    character.stats.refresh_inactive_pet_window = true;
                }
            }
            Ok::<(), ProcessPacketError>(())
        },
    });
}

/// UiInteractions the client sends when the HUD comes back after a full-screen
/// window closes: the chat window is re-created and reports its channel
/// ("ChatLogManager handleChannelChangeButton"), right after PetController:show().
/// Seen after closing both the store and Play Games.
pub fn is_hud_restored(window: &str, button: &str) -> bool {
    window == "ChatLogManager" && button == "handleChannelChangeButton"
}

/// Re-opens InactivePetWindow if a full-screen window hid it and no companion is
/// out.  The first-movement fallback in move_companion_if_active covers any
/// window that doesn't send the HUD-restored message.
pub fn reopen_inactive_pet_window_if_pending(sender: u32, game_server: &GameServer) -> Vec<Broadcast> {
    game_server
        .lock_enforcer()
        .read_characters(|_| CharacterLockRequest {
            read_guids: vec![],
            write_guids: vec![player_guid(sender)],
            character_consumer: |_, _, mut characters_write, _| {
                let Some(character) = characters_write.get_mut(&player_guid(sender)) else {
                    return Ok::<Vec<Broadcast>, ProcessPacketError>(vec![]);
                };
                if !character.stats.refresh_inactive_pet_window {
                    return Ok(vec![]);
                }
                character.stats.refresh_inactive_pet_window = false;
                if character.stats.companion_guid.is_some() {
                    return Ok(vec![]);
                }
                Ok(vec![Broadcast::Single(sender, reopen_inactive_pet_window())])
            },
        })
        .unwrap_or_default()
}

/// Force InactivePetWindow open again (Hide first in case the client still
/// thinks it is open but it was hidden with the HUD).
fn reopen_inactive_pet_window() -> Vec<Vec<u8>> {
    ["InactivePetWindow.Hide", "InactivePetWindow.Show"]
        .iter()
        .map(|name| {
            GamePacket::serialize(&TunneledPacket {
                unknown1: true,
                inner: ExecuteScriptWithIntParams {
                    script_name: name.to_string(),
                    params: vec![],
                },
            })
        })
        .collect()
}

/// Despawns the caller's active companion (if any) and restores `InactivePetWindow`.
/// Used by the `ClickDismissPetButton` UiInteraction handler.
pub fn despawn_active_companion(sender: u32, game_server: &GameServer) -> Vec<Broadcast> {
    game_server
        .lock_enforcer()
        .read_characters(|_| CharacterLockRequest {
            read_guids: vec![],
            write_guids: vec![player_guid(sender)],
            character_consumer: |characters_table_read_handle,
                                  _,
                                  mut characters_write,
                                  _| {
                let Some(character) = characters_write.get_mut(&player_guid(sender)) else {
                    return Ok::<Vec<Broadcast>, ProcessPacketError>(vec![]);
                };

                let Some(guid) = character.stats.companion_guid else {
                    return Ok(vec![]); // nothing to despawn
                };

                let Some((_, instance_guid, chunk)) =
                    characters_table_read_handle.index1(player_guid(sender))
                else {
                    return Ok(vec![]);
                };

                character.stats.companion_guid = None;
                character.stats.active_companion_item_guid = None;
                character.stats.companion_world_pos = None;
                let owned = if let CharacterType::Player(p) = &character.stats.character_type {
                    p.purchased_items.clone()
                } else {
                    Default::default()
                };
                let equipped = character.stats.companion_attachments.clone();

                crate::info!(
                    "Despawning companion guid={guid:#018x} for player {sender} (dismiss button)"
                );

                let all_players_nearby = ZoneInstance::all_players_nearby(
                    chunk,
                    instance_guid,
                    characters_table_read_handle,
                );

                Ok(vec![
                    Broadcast::Multi(
                        all_players_nearby,
                        vec![GamePacket::serialize(&TunneledPacket {
                            unknown1: true,
                            inner: RemoveGracefully {
                                guid,
                                use_death_animation: false,
                                delay_millis: 0,
                                composite_effect_delay_millis: 0,
                                composite_effect: 0,
                                fade_duration_millis: 1000,
                            },
                        })],
                    ),
                    Broadcast::Single(sender, set_active_pet_id(0, 0, &owned, &equipped)),
                ])
            },
        })
        .unwrap_or_default()
}
