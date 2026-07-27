use std::io::Cursor;

use packet_serialize::DeserializePacket;

use crate::game_server::{
    packets::{
        item::{BaseAttachmentGroup, WieldType},
        pet::{PetInventory, PetInventoryItem},
        player_update::{AddNpc, Hostility, Icon, PhysicsState, RemoveGracefully},
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
pub static COMPANION_ATTACHMENT_GROUPS: &[(u32, u32)] = &[
    // Protocol droids → base protocol droid parts (group 175: 311/430/447)
    // Additional packs: 512, 530, 550 — accessible via flyout category buttons
    (541,  175), // C-3PO
    (550,  175), // N0-80T
    (551,  175), // N-30H
    (552,  175), // J3-3V3
    (553,  175), // D-0T
    (998,  175), // RA-7
    // Techno-Service droid → Techno-Service Droid Part Pack (172, only one group)
    (633,  172), // T0-D0
    // Astromech droids → base astromech parts (group 174: 309/310/427)
    // Additional packs: 513, 529, 549, 635, 727 — accessible via flyout category buttons
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
    (3425, 174), // U9-C4
    // Mouse droids → base mouse droid parts (group 168: 424/444)
    // Additional packs: 551, 609, 682
    (542,  168), // F1-V3L
    (2092, 168), // 1M-AU5 (gold mouse droid)
    // Probe droid → Probe Droid Parts (171)
    (543,  171), // PR-0B07
    // Power droid → base power droid parts (group 173: 428/429/436)
    // Additional pack: 610
    (544,  173), // 5T-U85
    // AT-AT → base AT-AT parts (group 169: 563/564)
    // Additional pack: 713
    (632,  169), // 5P-0T
    // Creatures with actual gear groups (all others have no gear; gear icon stays gray)
    (2752, 641), // Aedalus  → Convor Gear (group 641: items 2715/2716)
    (3282, 837), // Phileas  → Kowakian Monkey Lizard Costumes (group 837: 3274/3275)
    (3283, 843), // Barnibus → Balloon Attachment (group 843: item 3285)
];

/// Maps companion item_guid → the attachment items to include in the `PetInventory` packet
/// (Pet sub_op 0x05).  Each entry is `(item_guid, item_type, icon_set_id)`.
///
/// `icon_set_id` maps to [item+0x1C] in the DataSource struct ("Icon ID" field).
/// Lua reads GetData("Icon ID") and passes it as `iconId` to
/// PetAttachmentListWindow:setAttachmentItem when the gear flyout opens.
///
/// `item_type` controls which sub-DataSource the item routes to (see `PetInventoryItem`
/// for the full mapping).  All attachment items here use item_type=1 (HeadData).
///
/// `instance_guid` is set equal to `template_id` since we track ownership by template ID.
pub static COMPANION_ATTACHMENT_ITEMS: &[(u32, &[(u32, u32, u32)])] = &[
    // Protocol droids → group 175: 311 (Serving Tray/266), 430 (Oil Slick/1184), 447 (Dance/1183)
    (541,  &[(311, 1,266), (430, 1,1184), (447, 1,1183)]),  // C-3PO
    (550,  &[(311, 1,266), (430, 1,1184), (447, 1,1183)]),  // N0-80T
    (551,  &[(311, 1,266), (430, 1,1184), (447, 1,1183)]),  // N-30H
    (552,  &[(311, 1,266), (430, 1,1184), (447, 1,1183)]),  // J3-3V3
    (553,  &[(311, 1,266), (430, 1,1184), (447, 1,1183)]),  // D-0T
    (998,  &[(311, 1,266), (430, 1,1184), (447, 1,1183)]),  // RA-7
    // Techno-Service droid → group 172: 560 (Repulsor/1432), 561 (Wheel/269), 562 (Explosive/268)
    (633,  &[(560, 1,1432), (561, 1,269), (562, 1,268)]),   // T0-D0
    // Astromech droids → group 174: 309 (Thruster/255), 310 (Party/1181), 427 (Pogo/258)
    (545,  &[(309, 1,255), (310, 1,1181), (427, 1,258)]),   // R2-D2
    (546,  &[(309, 1,255), (310, 1,1181), (427, 1,258)]),   // R3-S6
    (547,  &[(309, 1,255), (310, 1,1181), (427, 1,258)]),   // B3-3P5
    (548,  &[(309, 1,255), (310, 1,1181), (427, 1,258)]),   // M1-L0
    (549,  &[(309, 1,255), (310, 1,1181), (427, 1,258)]),   // B0-LT5
    (916,  &[(309, 1,255), (310, 1,1181), (427, 1,258)]),   // R2-KT
    (917,  &[(309, 1,255), (310, 1,1181), (427, 1,258)]),   // B3-T4
    (2146, &[(309, 1,255), (310, 1,1181), (427, 1,258)]),   // 6R-0WL
    (2147, &[(309, 1,255), (310, 1,1181), (427, 1,258)]),   // P3-NUT
    (2393, &[(309, 1,255), (310, 1,1181), (427, 1,258)]),   // H3-4RT
    (2804, &[(309, 1,255), (310, 1,1181), (427, 1,258)]),   // 7L-VN
    (3408, &[(309, 1,255), (310, 1,1181), (427, 1,258)]),   // M5-BZ
    (3425, &[(309, 1,255), (310, 1,1181), (427, 1,258)]),   // U9-C4
    // Mouse droids → group 168: 424 (Rocket/262), 444 (Bobblehead/192)
    (542,  &[(424, 1,262), (444, 1,192)]),                   // F1-V3L
    (2092, &[(424, 1,262), (444, 1,192)]),                   // 1M-AU5
    // Probe droid → group 171: 657 (UFO/1429), 658 (Jurassic/1427), 659 (Missile/1428)
    (543,  &[(657, 1,1429), (658, 1,1427), (659, 1,1428)]), // PR-0B07
    // Power droid → group 173: 428 (Generator/265), 429 (Boombox/264), 436 (Boxing/263)
    (544,  &[(428, 1,265), (429, 1,264), (436, 1,263)]),    // 5T-U85
    // Mini AT-AT → group 169: 563 (Spinners/254), 564 (Bipedal/1182)
    (632,  &[(563, 1,254), (564, 1,1182)]),                  // 5P-0T
    // Aedalus (Convor) → group 641: 2715 (Jetpack/1847), 2716 (Pilot/1848)
    (2752, &[(2715, 1,1847), (2716, 1,1848)]),               // Aedalus
    // Phileas (Kowakian) → group 837: 3274 (Sith Robe/2526), 3275 (Jedi Robe/2527)
    (3282, &[(3274, 1,2526), (3275, 1,2527)]),               // Phileas
    // Barnibus (Rancor) → group 843: 3285 (Sarlacc Balloon/2575)
    (3283, &[(3285, 1,2575)]),                                 // Barnibus
];

/// Maps companion item_guid → NPC model_id.
/// model_ids sourced from Models.txt.
pub static COMPANION_MODELS: &[(u32, u32)] = &[
    // Protocol droids
    (541,  164),  // C-3PO              (Char_ProtocolDroid3PO.adr)
    (550,  644),  // N0-80T             (3D_TC14.agr)
    (551,  646),  // N-30H              (3D_TC3.agr)
    (552,  647),  // J3-3V3             (3D_TC70.agr)
    (553,  645),  // D-0T               (3D_TC2.agr)
    (633,  353),  // T0-D0              (Char_TechnoServiceDroid.adr)
    (998,  1082), // RA-7               (Char_DeathStarDroidR7.adr)
    // Astromech droids
    (545,  150),  // R2-D2              (Char_AstroMechR2.adr)
    (546,  642),  // R3-S6              (3D_R3S6.agr)
    (547,  264),  // B3-3P5             (Char_AstroMechR4.adr)
    (548,  643),  // M1-L0              (3D_R4J1.agr)
    (549,  265),  // B0-LT5             (Char_AstroMechR5.adr)
    (916,  2220), // R2-KT              (3D_R2KT.agr)
    (917,  1104), // B3-T4              (3D_B3T4.agr)
    (2146, 1227), // 6R-0WL             (Grp_R2_Wampa.agr)
    (2147, 1319), // P3-NUT             (Grp_R2_Nutcracker.agr)
    (2393, 1408), // H3-4RT             (Grp_R2_Valentine.agr)
    (2804, 1622), // 7L-VN              (Grp_R2_711Promo.agr)
    // Mouse droids
    (542,  259),  // F1-V3L             (Char_MouseDroid.adr)
    (2092, 1099), // 1M-AU5             (Char_MouseDroidGold.adr)
    // Probe & power droids
    (543,  248),  // PR-0B07            (Char_ProbeDroid.adr)
    (544,  233),  // 5T-U85             (Char_PowerDroid.adr)
    // Other droids
    (554,  300),  // BR-3R              (Char_LEPServantDroid.adr)
    (632,  279),  // 5P-0T              (Char_ATAT.adr)
    (2114, 1142), // J-4CK              (Grp_R4_Pumpkin.agr)
    (2115, 1141), // K-3RNL             (Grp_R4_CandyCorn.agr)
    (3080, 2035), // DU-3               (Char_PitDroid.adr)
    (3081, 2060), // K0-5D              (Char_GladiatorDroid.adr)
    (3284, 2236), // BR-RR              (3D_BRRR.agr)
    // Creatures
    // Aedalus: Convor gear icons (1847-1848) are adjacent to Aedalus icon (1846)
    (2752, 1486), // Aedalus            (Char_Convor.adr)
    (2786, 1570), // Gnarls             (Char_Anoobus_LOD0.adr)
    (2802, 1612), // Eeetch             (Char_KowakianMonkeyLizard.adr)
    (2899, 1737), // Vector             (3D_Vector.agr)
    (3073, 1944), // Babbo              (3D_Babbo.agr)
    (3074, 1945), // Duppa              (3D_Duppa.agr)
    (3100, 2228), // Fang               (Char_Rancor.adr)
    (3219, 2161), // Erial              (3D_Convor_Purple.agr)
    // Phileas: Kowakian gear guids (3274-3275) adjacent to Phileas guid (3282)
    (3282, 2224), // Phileas            (Char_Sarlacc.adr)
    // Barnibus: Sarlacc Balloon gear icon (2575) adjacent to Barnibus icon (2573)
    (3283, 2225), // Barnibus           (Char_Rancor_Glow.adr)
    (3366, 2293), // Marrok             (Char_Marrok.adr)
    (3373, 2299), // Ghorroh            (Char_Gundark.adr)
    // Sidekicks
    (3079, 2031), // Aktik              (Holo_Jawa.adr)
    (3341, 2257), // Jynx               (Holo_NPCGungan.adr)
    (3398, 2315), // Klug               (Holo_GamorreanGuard.adr)
    // Missing companions (assigned GUIDs 3420-3447; models from Models.txt)
    // Creatures
    (3420, 2438), // Skreech            (3D_Skreech.agr)
    (3421, 2744), // Cinch              (Char_Tooka.adr)
    (3422, 2182), // Amelie             (Char_Millicreep.adr)
    (3423, 2470), // Gulsh              (Char_Narglatch.adr)
    (3424, 2470), // Kharroh            (Char_Narglatch.adr)
    (3415, 2470), // Gash               (Char_Narglatch.adr)
    (3406, 1612), // Cercopes           (Char_KowakianMonkeyLizard.adr)
    // Droids
    (3407, 2621), // WAC-47             (Char_PitDroidAsym.adr)
    (3408,  641), // M5-BZ              (3D_R2M5.agr)
    (3425, 2664), // U9-C4              (3D_Astromech_U9C4.agr)
    (3440, 2667), // BNI-393            (Char_LEPServantDroidBNI393.adr)
    (3446, 2745), // Security Battle Droid (Char_BattleDroid.adr)
    (3447, 2441), // L1-LR3D            (Char_HodgepodgeDroid.adr)
];

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
///   1. Hide `InactivePetWindow` via `PetController.hide`
///   2. Set the pet ID on `ActivePetWindow`
///   3. Set the attachment item group (enables/disables the gear icon)
///   4. Show `ActivePetWindow`
///
/// `companion_npc_guid` is the 64-bit NPC GUID of the spawned companion.
/// It is split into (lo, hi) u32 halves and placed in the trailing fields of
/// the PetInventory packet so the C++ pet system can route the DataSource
/// update to the correct companion.  Pass 0 when dismissing.
///
/// `pet_id == 0` reverses the process: hides `ActivePetWindow` and shows
/// `InactivePetWindow` directly (bypassing FTE).
pub fn set_active_pet_id(pet_id: u32, companion_npc_guid: u64) -> Vec<Vec<u8>> {
    if pet_id != 0 {
        // Look up attachment item group for gear icon — 0 means no gear (grayed out).
        let attachment_group = COMPANION_ATTACHMENT_GROUPS
            .iter()
            .find(|(guid, _)| *guid == pet_id)
            .map(|(_, g)| *g)
            .unwrap_or(0);

        // Look up attachment items for the PetInventory DataSource.
        let attachment_items: &[(u32, u32, u32)] = COMPANION_ATTACHMENT_ITEMS
            .iter()
            .find(|(guid, _)| *guid == pet_id)
            .map(|(_, items)| *items)
            .unwrap_or(&[]);

        // Steps 1-3 are always sent.
        let mut packets = vec![
            // Step 1: hide the "no pet" panel
            GamePacket::serialize(&TunneledPacket {
                unknown1: true,
                inner: ExecuteScriptWithIntParams {
                    script_name: "PetController.hide".to_string(),
                    params: vec![],
                },
            }),
            // Step 2a: register the active pet in the C++ PetController so that
            // PetInventory data routes to the correct companion. We previously
            // bypassed this because of the FTE guard that hides ActivePetWindow
            // when isFTEShown=false, but the C++ pet-system registration is needed
            // for PetInventory DataSource routing to work.
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
            // Step 3: show ActivePetWindow before setting the gear group, so the
            // call lands on a visible window.
            GamePacket::serialize(&TunneledPacket {
                unknown1: true,
                inner: ExecuteScriptWithIntParams {
                    script_name: "ActivePetWindow.Show".to_string(),
                    params: vec![],
                },
            }),
        ];

        let companion_guid_lo = companion_npc_guid as u32;
        let companion_guid_hi = (companion_npc_guid >> 32) as u32;

        // Step 4 (conditional): setPetAttachmentItemGroupId BEFORE PetInventory.
        // Hypothesis: the C++ pet system uses the group ID to set up routing state
        // that must be established before it will accept/process the PetInventory data.
        // SWF analysis shows the function name is lowercase on ActivePetView.
        // Calling with 0 hides ActivePetWindow entirely; only send for real groups.
        if attachment_group != 0 {
            crate::info!(
                "SetPetAttachmentItemGroupId pet_id={pet_id} group={attachment_group}"
            );
            for name in &[
                "ActivePetWindow.setPetAttachmentItemGroupId",
                "ActivePetWindow.SetPetAttachmentItemGroupId",
            ] {
                packets.push(GamePacket::serialize(&TunneledPacket {
                    unknown1: true,
                    inner: ExecuteScriptWithIntParams {
                        script_name: name.to_string(),
                        params: vec![attachment_group as i32],
                    },
                }));
            }

            // Step 5: PetInventory — fills BaseClient.PetInventory DataSource.
            // Sent after setPetAttachmentItemGroupId so the group routing state is
            // established first.  Trailing 3×u32 carry companion NPC GUID (lo/hi)
            // and pet item ID.
            if !attachment_items.is_empty() {
                let pet_inventory_packet = GamePacket::serialize(&TunneledPacket {
                    unknown1: true,
                    inner: PetInventory {
                        items: attachment_items
                            .iter()
                            .map(|&(item_guid, item_type, icon_set_id)| PetInventoryItem {
                                instance_guid: item_guid,
                                template_id: item_guid,
                                item_type,
                                name: String::new(),
                                tint_value: icon_set_id,
                            })
                            .collect(),
                        companion_guid_lo,
                        companion_guid_hi,
                        pet_item_id: pet_id,
                    },
                });
                crate::info!(
                    "PetInventory pet_id={pet_id} ({} items, types={:?}): len={} hex={:02x?}",
                    attachment_items.len(),
                    attachment_items.iter().map(|(_, t, _)| t).collect::<Vec<_>>(),
                    pet_inventory_packet.len(),
                    &pet_inventory_packet[..pet_inventory_packet.len().min(80)],
                );
                packets.push(pet_inventory_packet);
            }

            // Step 6: Pre-populate the flyout using the root-delegate names that
            // AttachmentSelectionView's constructor registers on _root (confirmed via
            // AS2 bytecode decode of tag[312] in PetAttachmentListWindow.swf):
            //
            //   _root.clearAttachmentItems()                    → _clearAttachmentItems()
            //   _root.setAttachmentItem(guid,itemId,name,iconId)→ _handleAddAttachmentItem()
            //   _root.addAttachmentFinished()                   → _handleAddAttachmentFinished()
            //
            // addAttachmentFinished inserts ui_None at front + BuyAttachment at end,
            // then calls invalidateAttachmentView() which drives addItem() on the grid.
            // The gear button opens the flyout client-side (no server round-trip), so
            // we must push items now (at summon time) rather than on-demand.
            // ExecuteScriptWithIntParams only carries i32 params; name is sent as 0.
            for window in &["ActivePetWindow", "PetAttachmentListWindow"] {
                packets.push(GamePacket::serialize(&TunneledPacket {
                    unknown1: true,
                    inner: ExecuteScriptWithIntParams {
                        script_name: format!("{window}.clearAttachmentItems"),
                        params: vec![],
                    },
                }));
                for &(item_guid, _item_type, icon_set_id) in attachment_items {
                    packets.push(GamePacket::serialize(&TunneledPacket {
                        unknown1: true,
                        inner: ExecuteScriptWithIntParams {
                            // setAttachmentItem(guid, itemId, name, iconId)
                            script_name: format!("{window}.setAttachmentItem"),
                            params: vec![item_guid as i32, item_guid as i32, 0, icon_set_id as i32],
                        },
                    }));
                }
                packets.push(GamePacket::serialize(&TunneledPacket {
                    unknown1: true,
                    inner: ExecuteScriptWithIntParams {
                        script_name: format!("{window}.addAttachmentFinished"),
                        params: vec![],
                    },
                }));
            }
        } else {
            crate::info!(
                "SetPetAttachmentItemGroupId pet_id={pet_id}: no group mapping, gear icon stays grayed out"
            );
        }

        packets
    } else {
        vec![
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
        ]
    }
}

/// Sends `PetInventory` + `SetPetAttachmentItemGroupId` + the three direct Lua
/// calls that populate the attachment flyout for `pet_id`.
///
/// The flyout (ActivePetWindow/PetAttachmentListWindow, AttachmentSelectionView) exposes
/// three Lua-callable root-delegates (confirmed via AS2 bytecode decode):
///   clearAttachmentItems()                     — resets m_attachmentItems to []
///   setAttachmentItem(guid,itemId,name,iconId) — pushes one item to m_attachmentItems
///   addAttachmentFinished()                    — adds ui_None+BuyAttachment, then renders
/// PetInventory is kept for its other uses (currently-equipped tracking).
///
/// Called from ClickBuyAttachmentPetButton and from the ClickSummonPetButton
/// resend path via set_active_pet_id.
/// Does NOT re-run the window show/hide sequence so the panel doesn't flash.
pub fn resend_pet_inventory(pet_id: u32, companion_npc_guid: u64) -> Vec<Vec<u8>> {
    let attachment_items: &[(u32, u32, u32)] = COMPANION_ATTACHMENT_ITEMS
        .iter()
        .find(|(guid, _)| *guid == pet_id)
        .map(|(_, items)| *items)
        .unwrap_or(&[]);

    let attachment_group = COMPANION_ATTACHMENT_GROUPS
        .iter()
        .find(|(guid, _)| *guid == pet_id)
        .map(|(_, g)| *g)
        .unwrap_or(0);

    let companion_guid_lo = companion_npc_guid as u32;
    let companion_guid_hi = (companion_npc_guid >> 32) as u32;

    let mut packets = Vec::new();

    // PetInventory first — fills BaseClient.PetInventory DataSource.
    // SetPetAttachmentItemGroupId reads from it; must arrive first.
    packets.push(GamePacket::serialize(&TunneledPacket {
        unknown1: true,
        inner: PetInventory {
            items: attachment_items
                .iter()
                .map(|&(item_guid, item_type, icon_set_id)| PetInventoryItem {
                    instance_guid: item_guid,
                    template_id: item_guid,
                    item_type,
                    name: String::new(),
                    tint_value: icon_set_id,
                })
                .collect(),
            companion_guid_lo,
            companion_guid_hi,
            pet_item_id: pet_id,
        },
    }));

    // setPetAttachmentItemGroupId second — tells the C++ which group to show.
    // SWF uses lowercase; send both cases for safety.
    if attachment_group != 0 {
        for name in &[
            "ActivePetWindow.setPetAttachmentItemGroupId",
            "ActivePetWindow.SetPetAttachmentItemGroupId",
        ] {
            packets.push(GamePacket::serialize(&TunneledPacket {
                unknown1: true,
                inner: ExecuteScriptWithIntParams {
                    script_name: name.to_string(),
                    params: vec![attachment_group as i32],
                },
            }));
        }

        // Directly populate the flyout using the root-delegate names that
        // AttachmentSelectionView's constructor actually registers on _root
        // (confirmed via AS2 bytecode decode of tag[312] in PetAttachmentListWindow.swf):
        //
        //   _root.clearAttachmentItems()                    → _clearAttachmentItems()
        //   _root.setAttachmentItem(guid,itemId,name,iconId)→ _handleAddAttachmentItem()
        //   _root.addAttachmentFinished()                   → _handleAddAttachmentFinished()
        //
        // ExecuteScriptWithIntParams can only send i32 values; name is passed as 0.
        // The SWF is loaded in both windows; send to both.
        for window in &["ActivePetWindow", "PetAttachmentListWindow"] {
            packets.push(GamePacket::serialize(&TunneledPacket {
                unknown1: true,
                inner: ExecuteScriptWithIntParams {
                    script_name: format!("{window}.clearAttachmentItems"),
                    params: vec![],
                },
            }));
            for &(item_guid, _item_type, icon_set_id) in attachment_items {
                packets.push(GamePacket::serialize(&TunneledPacket {
                    unknown1: true,
                    inner: ExecuteScriptWithIntParams {
                        // setAttachmentItem(guid, itemId, name, iconId)
                        script_name: format!("{window}.setAttachmentItem"),
                        params: vec![item_guid as i32, item_guid as i32, 0, icon_set_id as i32],
                    },
                }));
            }
            packets.push(GamePacket::serialize(&TunneledPacket {
                unknown1: true,
                inner: ExecuteScriptWithIntParams {
                    script_name: format!("{window}.addAttachmentFinished"),
                    params: vec![],
                },
            }));
        }
    }

    packets
}

pub fn spawn_companion_npc(
    companion_guid: u64,
    model_id: u32,
    name_id: u32,
    spawn_pos: Pos,
    spawn_rot: Pos,
    attachment_group: u32,
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
            attachments: vec![],
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
            interactable_size_pct: 0,
            walk_animation_id: -1,
            sprint_animation_id: -1,
            stand_animation_id: 1,
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
            clickable: false,
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
            disable_move_to_interact: false,
            unknown69: 0.0,
            unknown70: 0.0,
            unknown71: 0,
            icon_id: Icon::None,
        },
    })]
}

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
    if cmd == 0x00 {
        return Ok(vec![]);
    }
    let item_guid: u32 = DeserializePacket::deserialize(cursor).unwrap_or(0);
    let _summon: u8 = DeserializePacket::deserialize(cursor).unwrap_or(0);

    crate::info!("Companion packet from {sender}: cmd={cmd:#04x} item_guid={item_guid}");

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

                // Verify the requested companion belongs to this player.
                let owns_companion = {
                    if let CharacterType::Player(player) = &character.stats.character_type {
                        player.companions.contains(&item_guid)
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
                    if active_item == Some(item_guid) {
                        // Same companion — toggle off and return.
                        return Ok(vec![
                            Broadcast::Multi(all_players_nearby, vec![remove_pkt]),
                            Broadcast::Single(sender, set_active_pet_id(0, 0)),
                        ]);
                    }
                    // Different companion — stash the remove packet and fall through to spawn.
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

                let companion_guid = pet_guid(player_guid(sender));
                character.stats.companion_guid = Some(companion_guid);
                character.stats.active_companion_item_guid = Some(item_guid);

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

                let spawn_attachment_group = COMPANION_ATTACHMENT_GROUPS
                    .iter()
                    .find(|(guid, _)| *guid == item_guid)
                    .map(|(_, g)| *g)
                    .unwrap_or(0);
                let mut spawn_packets =
                    spawn_companion_npc(companion_guid, model_id, 0, companion_spawn_pos, player_rot, spawn_attachment_group);
                // If we're switching companions, prepend the old one's removal so both
                // happen in the same broadcast (no visual gap).
                if let Some(remove_pkt) = despawn_packet {
                    spawn_packets.insert(0, remove_pkt);
                }

                Ok(vec![
                    Broadcast::Multi(all_players_nearby, spawn_packets),
                    // Show ActivePetWindow directly, bypassing PetController.SetActivePetId
                    // (which has an FTE guard that blocks the window when isFTEShown=false).
                    // We mirror what PetsHandler_PetIsActiveUpdate does natively:
                    // hide InactivePetWindow, set pet ID, show ActivePetWindow.
                    // Pass companion_guid so PetInventory trailing u32s carry the NPC GUID.
                    Broadcast::Single(sender, set_active_pet_id(item_guid, companion_guid)),
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
                    Broadcast::Single(sender, set_active_pet_id(0, 0)),
                ])
            },
        })
        .unwrap_or_default()
}
