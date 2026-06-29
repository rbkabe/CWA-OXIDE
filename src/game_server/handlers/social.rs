use std::io::Cursor;

use packet_serialize::DeserializePacket;

use crate::game_server::{
    packets::{
        item::EquipmentSlot,
        social::{
            EquippedItemEntry, ProfileGearEntry, ProfileItemEntry, SocialOpCode,
            SocialPacketProfileInfo, SocialPacketRequestProfile,
        },
        tunnel::TunneledPacket,
        GamePacket,
    },
    Broadcast, GameServer, ProcessPacketError, ProcessPacketErrorType,
};

/// TEST 9: send our internal `EquipmentSlot` repr value as-is for the 6
/// gear-box slots; drop sabers (no UI box exists for them).
///
/// TEST 8's "remap to compact 0-5" was a guess and was WRONG. This session,
/// `ProfileWindow.swf` was available pre-extracted (decrypted assets, no
/// pack-chasing needed) and was fully disassembled (custom AVM1
/// disassembler -- no ffdec/network access in this environment) down to the
/// literal `_isValidSlot` bytecode on the `GearView` class. The function
/// body is a flat `if/else` chain of `StrictEquals` checks, decompiling to:
///
/// ```as2
/// function _isValidSlot(slot) {
///     if (slot === 1) return true; // Head
///     if (slot === 2) return true; // Hands
///     if (slot === 3) return true; // Body
///     if (slot === 4) return true; // Feet
///     if (slot === 7) return true; // PrimaryWeapon
///     if (slot === 8) return true; // SecondaryWeapon
///     return false;
/// }
/// ```
///
/// `NUM_SLOTS` (=8, also confirmed in bytecode) is only a loop bound for
/// `_resetSlots`/`_getSlot`, not the valid-ID set -- the actual gate is this
/// exact `{1,2,3,4,7,8}` literal set, which is precisely our original
/// (pre-TEST-8) `EquipmentSlot` repr for non-saber gear. So no remapping is
/// needed at all: pass the raw repr value through and only filter out slots
/// `_isValidSlot` would reject (sabers, `None`).
///
/// If the Gear tab is still blank after this fix, the bug is NOT slot-value
/// rejection -- the next thing to check is whether an earlier field in
/// `SocialPacketProfileInfo`/`ProfileGearEntry` has the wrong size/order,
/// misaligning the parse before it ever reaches the Slot check (suspected
/// after TEST 7, which sent valid+invalid slots together and still failed).
fn gear_ui_slot(slot: EquipmentSlot) -> Option<u32> {
    match slot {
        EquipmentSlot::Head
        | EquipmentSlot::Hands
        | EquipmentSlot::Body
        | EquipmentSlot::Feet
        | EquipmentSlot::PrimaryWeapon
        | EquipmentSlot::SecondaryWeapon => Some(u32::from(slot)),
        _ => None,
    }
}

use super::{character::CharacterType, lock_enforcer::CharacterLockRequest, unique_guid::player_guid};

fn process_request_profile(
    cursor: &mut Cursor<&[u8]>,
    sender: u32,
    game_server: &GameServer,
) -> Result<Vec<Broadcast>, ProcessPacketError> {
    let request = SocialPacketRequestProfile::deserialize(cursor)?;

    // TODO: support requesting other players' profiles once the target lookup
    // (likely via the guid -> name/location index) is wired up. For now we only
    // ever answer with the requester's own data, which is sufficient for
    // self-profile testing.
    let _ = request.target_guid;

    game_server
        .lock_enforcer()
        .read_characters(|_| CharacterLockRequest {
            read_guids: vec![player_guid(sender)],
            write_guids: Vec::new(),
            character_consumer: move |_, characters_read, _, _| {
                let Some(character) = characters_read.get(&player_guid(sender)) else {
                    return Err(ProcessPacketError::new(
                        ProcessPacketErrorType::ConstraintViolated,
                        format!("Non-existent player {sender} requested their own profile"),
                    ));
                };

                let CharacterType::Player(player) = &character.stats.character_type else {
                    return Err(ProcessPacketError::new(
                        ProcessPacketErrorType::ConstraintViolated,
                        format!(
                            "(Requester: {sender}) requested a profile but is a non-player character"
                        ),
                    ));
                };

                // TEST 1-9 all FAILED. TEST 9 confirmed (via disassembled
                // `_isValidSlot` bytecode) that the valid-ID set is
                // {1,2,3,4,7,8}, our raw un-remapped `EquipmentSlot` repr --
                // and the server log confirmed those exact values (1,2,3,4,7)
                // were sent on the wire. So slot-ID rejection is ruled out as
                // the (sole) cause.
                //
                // Spent considerable effort this session trying to find hard
                // evidence (via the client's `ScriptsBase.bin` Lua bytecode
                // and reflection-style string tables in `CloneWars.exe`) of
                // which wire field -- `unknown1` or `unknown2` -- the client
                // binds to the "Slot" column vs. the "ItemId" column that
                // `GearView` actually reads. Dead end: `ScriptsBase.bin` only
                // contains a generic DS-name -> property-path registration
                // (`DS_SELECTED_PROFILE_GEAR` -> `BaseClient.SelectedSocialProfile.Gear`),
                // with no per-column field-order info. The only standalone
                // "ItemId"/"Slot" reflection strings found in the binary
                // belong to unrelated systems (store items, ability
                // definitions) -- not `ProfileGearEntry`. And the
                // "EquippedItemId"/"EquippedItemSlot" strings found earlier
                // belong to the live equipment-broadcast system, confirmed
                // not this one.
                //
                // TEST 10: swapped `unknown1`/`unknown2` (item_id/slot order)
                // -- FAILED, same as TEST 9. Confirmed via screenshot
                // evidence this round that the overall packet IS valid and
                // fully parsed (Trophies tab correctly displays the account
                // name "rbkabe" pulled straight from our `name` field; every
                // other tab shows the client's normal "no data" placeholder,
                // not a parse-failure/blank state). So the bug is provably
                // isolated to the Gear array's *content* not landing in
                // `GearView`, independent of unknown1/unknown2 order -- both
                // orderings produced empty gear boxes (not even a
                // placeholder), unlike every other (genuinely empty) array.
                //
                // TEST 11: moved slot into `unknown3` (zeroing
                // unknown1/unknown2) -- FAILED, identical to TEST 9/10.
                // Three different field placements inside `ProfileGearEntry`
                // (unknown1, unknown2, unknown3) all produced the exact same
                // zero-visible-effect result. If we were even hitting the
                // array `GearView` reads, shuffling values around inside its
                // elements should have changed *something* by now. That's
                // strong circumstantial evidence `unknown_array2` (field 5,
                // offset 0xa0) simply isn't the array bound to
                // `DS_SELECTED_PROFILE_GEAR` at all -- our TEST 6 guess about
                // *which* field is Gear, not just its internal layout, may be
                // wrong.
                //
                // Confirmed separately (in-game) that the equipment
                // inventory screen ("MY CHARACTER" -> Gear) correctly shows
                // Head/Body/Hands/Feet/Right Hand from this same
                // `equipped_items()` data -- but that screen is driven by the
                // live equipment-broadcast packets, not
                // `SocialPacketProfileInfo`. So the underlying item/slot data
                // and `gear_ui_slot` mapping are confirmed correct; the bug
                // is specific to this packet's field layout.
                //
                // TEST 12 (field 4) and TEST 13 (field 16) both FAILED.
                //
                // TEST 14: disassembling the two previously-unimplemented
                // sub-structure fields (`FUN_00abdd30`/`FUN_00abb650`)
                // revealed they are NOT flat `Vec<u32>`s like we'd assumed --
                // each is a count-prefixed array of (key: u32, name: String,
                // value1: u32, value2: u32) entries, built via a real
                // map-style insert (teardown-then-repopulate), unlike every
                // plain array field tested so far. This is a much closer
                // structural match for "item id + name + a property" than
                // anything we've tried. Populate `unknown_array12` (field
                // 19, via `FUN_00abdd30`) with one `ProfileItemEntry` per
                // equipped item: key = item id, value1 = `gear_ui_slot`
                // (dropping sabers, same valid-slot filter confirmed via
                // `GearView::_isValidSlot` in TEST 9), name left empty since
                // we don't have item display names wired up yet.
                // TEST 15: found the ACTUAL Gear data source class via Ghidra --
                // `SocialPlayerEquippedItemsDataSource` (RTTI-confirmed: string
                // table hit on its mangled name, then its constructor and
                // `vfunction2_for_IDataAccessTable` decompiled directly). Its
                // column-name switch returns exactly TWO columns: 0 = "Slot",
                // 1 = "ItemId" -- no name, no extra properties. Its constructor
                // also takes two separate params, stored independently
                // (field_0x68/field_0x6c) rather than one combined pointer --
                // consistent with a column-oriented (SoA) table built from TWO
                // PARALLEL flat `Vec<u32>` arrays zipped by row index, not a
                // single array-of-structs. A fundamentally different shape than
                // every array-of-structs/map hypothesis tried in TEST 1-14.
                //
                // This retroactively explains every prior failure: TEST 1-14
                // each populated exactly one field while leaving everything else
                // empty. If Gear needs a *matched pair* of equal-length arrays,
                // every previous test produced one populated array paired
                // against an empty one -- zero usable rows -- regardless of
                // which field we chose or how its elements were shaped.
                //
                // Field 18 (`gear_items`) is already a flat `Vec<u32>` of item
                // IDs -- the natural fit for "ItemId". The adjacent,
                // still-untested field 17 (`unknown_array11`, offset 0x134) is
                // the best candidate for "Slot": same plain-`Vec<u32>` shape,
                // structurally adjacent to field 18 in the wire layout. Populate
                // both in lockstep (same order, same length, sabers dropped from
                // both together via `gear_ui_slot`) so they zip correctly by row
                // index.
                // TEST 16: field 20 (`unknown_array13`) is the OTHER map-style
                // structure (`FUN_00abb650`), and its per-element value function
                // `FUN_00ab7c30` reads 4 plain bounds-checked `u32`s -- no string,
                // unlike field 19. Combined with the `u32` key the outer loop
                // reads itself, each entry is `(key, value1, value2, value3,
                // value4)`. TEST 15 (two separate parallel `Vec<u32>` columns for
                // the confirmed "Slot"/"ItemId" schema of
                // `SocialPlayerEquippedItemsDataSource`) FAILED, so try encoding
                // both columns together as one map entry instead: key = Slot,
                // value1 = ItemId, value2-4 = 0 (unused).
                // TEST 16 FAILED differently from every prior test: the
                // Trophies tab disappeared entirely once unknown_array13
                // carried real `EquippedItemEntry` data, even though the
                // wire layout was confirmed BYTE-FOR-BYTE correct by
                // disassembling both `FUN_00abb650` (outer loop) and
                // `FUN_00ab7c30` (per-element value reader):
                //   - `FUN_00abb650` tears down the existing map, reads a
                //     `u32` count, then per entry reads a `u32` key directly
                //     (advancing the cursor itself), calls
                //     `FUN_00aba670(&key)` (map insert/lookup by key only --
                //     no buffer read), then calls `FUN_00ab7c30(cursor)`
                //     (implicit thiscall `this` = the just-inserted entry).
                //   - `FUN_00ab7c30` reads exactly 4 plain bounds-checked
                //     `u32`s into that entry's 4 slots -- no string, no
                //     nested structure.
                // So the 20-bytes-per-entry layout we sent in TEST 16
                // matches exactly; this rules out a parse/truncation bug.
                // The failure is therefore downstream of parsing: a VALUES
                // problem in the Gear data binding, not a wire-format
                // problem. The two confirmed display columns ("Slot",
                // "ItemId") almost certainly map to the first two values
                // `FUN_00ab7c30` reads (entry[0], entry[1]) -- NOT to the
                // outer loop's `key`, which in a `HashListMap` is typically
                // a unique per-row insert key, unrelated to either displayed
                // column. TEST 16 used `key = slot`, which may have
                // collided with something the map-insert code expected to
                // be unique/sequential, throwing during bind and aborting
                // the rest of the Profile window's tab-creation (taking
                // Trophies down with it).
                //
                // TEST 17: keep the proven-correct wire layout, change the
                // semantics -- key = sequential row index (0..n, guaranteed
                // unique), entry[0] (value1) = Slot, entry[1] (value2) =
                // ItemId, entry[2..3] (value3/value4) = 0.
                // TEST 17 FAILED identically to TEST 16 -- changing the
                // outer `key` from slot value to a sequential row index
                // made zero observable difference (Gear still blank,
                // Trophies still gone). Since the wire layout is proven
                // byte-correct via direct disassembly of both
                // `FUN_00abb650` and `FUN_00ab7c30`, and the symptom doesn't
                // change with the VALUES we put in, the crash/breakage is
                // triggered by populating field 20 with ANY non-empty data
                // at all, not by which field/value mapping we choose. This
                // suggests either (a) field 20 isn't Gear's array and we're
                // corrupting/crashing some unrelated system that also reads
                // it, or (b) there's a bind-time crash in the Gear path
                // itself that isn't sensitive to content. Reverting to
                // empty again; next step is to get ground truth on the
                // actual GearView bind/refresh function (the AS2 code that
                // reads rows out of the DataSource and populates slot
                // icons) rather than continue permuting field values.
                let battle_class = player.inventory.active_battle_class;
                let equipped = player.inventory.equipped_items(battle_class);

                // TEST 18: every plain Vec<u32> field and every sub-structure
                // field tried so far (TESTS 1-17) has failed, regardless of
                // which slot/item-id encoding we used. `unknown_array6`
                // (field 10, struct offset 0xd4, read via `FUN_00abc7e0`) is
                // the one plain count-prefixed array field that has NOT been
                // tried in isolation yet. Isolate a single, visually
                // unambiguous test case -- just the Head slot's equipped
                // item (the Shadow Tech Helmet, if that's what's currently
                // equipped) -- as a single-element array, so a Gear tab
                // change is unambiguous to spot in-game and not confused
                // with noise from other slots.
                // TEST 18 FAILED (confirmed via server log: head_item_guid=
                // Some(3276), packet sent correctly, Gear tab still didn't
                // populate).
                //
                // TEST 19: `unknown_bool` (field 11, struct offset 0x1b0) has
                // been sent as `false` in every single prior test. Unlike
                // every other field, this is the only boolean in the whole
                // struct -- a strong candidate for a "has gear data"/"gear
                // tab enabled" gate rather than just another unknown value.
                // If the client checks this flag before even looking at any
                // array field, that would explain why every single-field
                // permutation in TESTS 1-18 failed identically: the gate was
                // always off, so the array contents never mattered. Flip it
                // to `true` while keeping the same TEST 18 data
                // (unknown_array6 = [head item guid]) so we change exactly
                // one new variable.
                let head_item_guid = equipped.get(&EquipmentSlot::Head).copied();
                let gear_test_array: Vec<u32> = head_item_guid.into_iter().collect();
                let gear_data_present = !gear_test_array.is_empty();

                let gear_map_entries: Vec<EquippedItemEntry> = Vec::new();
                let gear_slots: Vec<u32> = Vec::new();
                let gear_items: Vec<u32> = Vec::new();
                let gear_id_list: Vec<u32> = Vec::new();
                let gear_item_entries: Vec<ProfileItemEntry> = Vec::new();
                let gear_entries: Vec<ProfileGearEntry> = Vec::new();

                crate::debug!(
                    "RequestProfile from {sender}: battle_class={battle_class}, head_item_guid={head_item_guid:?}, sending ProfileInfo unknown_array6={gear_test_array:?}, unknown_bool={gear_data_present}"
                );

                let serialized = GamePacket::serialize(&TunneledPacket {
                    unknown1: true,
                    inner: SocialPacketProfileInfo {
                        guid: player_guid(sender),
                        name: player.name.to_string().into_bytes(),
                        unknown1: 0,
                        unknown_array1: gear_id_list,
                        unknown_array2: gear_entries,
                        unknown_array3: Vec::new(),
                        unknown2: 0,
                        unknown_array4: Vec::new(),
                        unknown_array5: Vec::new(),
                        unknown_array6: gear_test_array,
                        unknown_bool: gear_data_present,
                        unknown_array7: Vec::new(),
                        unknown_array8: Vec::new(),
                        unknown3: 0,
                        unknown_array9: Vec::new(),
                        unknown_array10: Vec::new(),
                        unknown_array11: gear_slots,
                        gear_items,
                        unknown_array12: gear_item_entries,
                        unknown_array13: gear_map_entries,
                        trailing_unknown: 0,
                    },
                });

                // TEST 7 (sending the complete 21-field struct) ALSO FAILED
                // -- ruling out leftover-bytes/truncation as the (sole)
                // gate, or meaning an earlier field's shape is still wrong
                // and misaligning everything after it. Dump the exact bytes
                // we send so we can hand-verify our own serialization
                // against the disassembled field offsets/sizes before
                // assuming the layout itself is still broken.
                crate::debug!(
                    "RequestProfile from {sender}: serialized ProfileInfo packet ({} bytes): {:02x?}",
                    serialized.len(),
                    serialized
                );

                Ok(vec![Broadcast::Single(sender, vec![serialized])])
            },
        })
}

pub fn process_social_packet(
    cursor: &mut Cursor<&[u8]>,
    sender: u32,
    game_server: &GameServer,
) -> Result<Vec<Broadcast>, ProcessPacketError> {
    let raw_op_code: u16 = DeserializePacket::deserialize(cursor)?;
    match SocialOpCode::try_from(raw_op_code) {
        Ok(op_code) => match op_code {
            SocialOpCode::RequestProfile => process_request_profile(cursor, sender, game_server),
            _ => {
                let remaining = &cursor.get_ref()[cursor.position() as usize..];
                Err(ProcessPacketError::new(
                    ProcessPacketErrorType::UnknownOpCode,
                    format!(
                        "Unimplemented social op code: {op_code:?}, remaining bytes: {remaining:x?}"
                    ),
                ))
            }
        },
        Err(_) => {
            let remaining = &cursor.get_ref()[cursor.position() as usize..];
            Err(ProcessPacketError::new(
                ProcessPacketErrorType::UnknownOpCode,
                format!("Unknown social op code: {raw_op_code}, remaining bytes: {remaining:x?}"),
            ))
        }
    }
}
