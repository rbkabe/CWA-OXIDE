use num_enum::TryFromPrimitive;
use packet_serialize::{DeserializePacket, SerializePacket};

use super::{GamePacket, OpCode};

/// Sub-opcodes under `OpCode::Social` (0xa2), as confirmed via Ghidra analysis of
/// the client's `ClientSocialProfileManager`/`BaseSocialPacket` hierarchy and a
/// live packet capture of a real `RequestProfile` from the client.
///
/// Only `RequestProfile` and `ProfileInfo` are confirmed/implemented so far.
/// The client dispatcher (`vfunction25_for_ClientGatewayHandler` ->
/// `FUN_00ac4190`) also routes sub-opcodes 6, 10, 11, 12, 13, 25 (0x19),
/// 26 (0x1a), 33 (0x21), 34 (0x22), 35 (0x23) to separate handlers -- these are
/// presumably the other Social* packet types (Friends, Trophies, Scores, etc.)
/// and are not yet implemented here.
#[derive(Copy, Clone, Debug, TryFromPrimitive)]
#[repr(u16)]
pub enum SocialOpCode {
    ProfileInfo = 0x1,
    RequestProfile = 0x5,
}

impl SerializePacket for SocialOpCode {
    fn serialize(&self, buffer: &mut Vec<u8>) {
        OpCode::Social.serialize(buffer);
        (*self as u16).serialize(buffer);
    }
}

/// Client -> server request to open a player's profile.
///
/// Confirmed via live capture: 16 bytes total on the wire after the two opcode
/// u16s -- a u32 target player GUID followed by 8 bytes of padding/unused data
/// (observed as all zero when requesting one's own profile).
#[derive(DeserializePacket)]
pub struct SocialPacketRequestProfile {
    pub target_guid: u32,
    pub unknown: u64,
}

/// Per-element structure for `SocialPacketProfileInfo::unknown_array2` (field
/// 5, read via `FUN_00abde60` -> per-element sub-deserializer chain
/// `FUN_00abba70` -> (vtable dispatch) -> `FUN_00aba210`).
///
/// Disassembled `FUN_00aba210` instruction-by-instruction (TEST 6): each
/// element copies 8 raw bytes into the destination object at offsets
/// 0x28/0x2c (a contiguous `u64`), then immediately reads a length-prefixed
/// string at offset 0x30 via the *same* `0x748020` call used for the parent
/// packet's `name` field (i.e. identical wire shape: `u32 len` + bytes), then
/// reads 5 more plain `u32` fields at offsets 0x64, 0x68, 0x6c, 0x70, 0x74
/// (each independently bounds-checked/truncate-safe like every other field
/// in this packet). This is a strong match for a generic "named entry with a
/// few stat fields" shape -- exactly what a Gear list entry would need (an
/// id, a display name, and a handful of numeric properties like slot/item
/// id). This sub-structure was never previously populated with real data
/// (only ever sent as an empty array, on the assumption count=0 was a safe
/// placeholder) even though TEST 1-5 exhausted every plain `Vec<u32>`
/// position elsewhere in the packet.
#[derive(SerializePacket)]
pub struct ProfileGearEntry {
    pub id: u64,
    pub name: String,
    pub unknown1: u32,
    pub unknown2: u32,
    pub unknown3: u32,
    pub unknown4: u32,
    pub unknown5: u32,
}

/// Per-element structure for `SocialPacketProfileInfo::unknown_array12`
/// (field 19, struct offset 0x164, read via `FUN_00abdd30`).
///
/// TEST 14: disassembled `FUN_00abdd30`/`FUN_00aba8c0` instruction-by-
/// instruction. Unlike every plain `Vec<u32>` field, this one tears down an
/// existing map/list before repopulating (`FUN_00abc5d0` loop) and reads
/// each element as: a `u32` key (read by `FUN_00abdd30` itself, used as a
/// map lookup/insert key via `FUN_00abb890`), then (inside `FUN_00aba8c0`) a
/// length-prefixed string (name) via the same `0x748020` call as every other
/// string field, then two more plain `u32`s at offsets 0x34/0x38. So each
/// entry is `(key: u32, name: String, value1: u32, value2: u32)` -- a
/// smaller, more specific "named entry" shape than `ProfileGearEntry`
/// (id+name+5 numbers), and a much better structural fit for "item id + a
/// couple of properties (e.g. slot)" than a bare `Vec<u32>`. Never tested
/// until now.
#[derive(SerializePacket)]
pub struct ProfileItemEntry {
    pub key: u32,
    pub name: String,
    pub value1: u32,
    pub value2: u32,
}

/// Per-element structure for `SocialPacketProfileInfo::unknown_array13`
/// (field 20, struct offset 0x17c, read via `FUN_00abb650`/`FUN_00ab7c30`).
///
/// TEST 16: disassembled `FUN_00ab7c30` (field 20's per-element value
/// deserializer, the counterpart to field 19's `FUN_00aba8c0`) and found a
/// totally different shape: 4 plain bounds-checked `u32` reads, NO string.
/// Combined with the `u32` key read by the outer `FUN_00abb650` loop itself
/// (same map-style teardown-then-repopulate pattern as field 19), each entry
/// is `(key: u32, value1: u32, value2: u32, value3: u32, value4: u32)`.
///
/// This is a strong match for the real Gear data source we found via Ghidra
/// RTTI/string-table search this round: `SocialPlayerEquippedItemsDataSource`,
/// whose `IDataAccessTable` column schema is exactly two columns -- "Slot"
/// (0) and "ItemId" (1) -- and which is backed (per nearby RTTI strings) by a
/// `HashListMap<Social::EquippedItem>`, i.e. a real keyed map, not a flat
/// array. TEST 15 (two separate parallel `Vec<u32>` columns in fields 17/18)
/// FAILED, so the two columns are likely encoded together as one map's
/// key/value here instead. key = Slot (the natural lookup key for a small
/// fixed slot set, mirroring field 19's key-as-lookup-id usage), value1 =
/// ItemId, value2-4 = 0 (every other field has tolerated unused trailing
/// zeros so far).
#[derive(SerializePacket)]
pub struct EquippedItemEntry {
    pub key: u32,
    pub value1: u32,
    pub value2: u32,
    pub value3: u32,
    pub value4: u32,
}

/// Server -> client response containing profile data.
///
/// Field layout fully re-derived this round by disassembling the actual
/// client binary (`CloneWars.exe`, objdump on the PE) at `FUN_00ac2260`
/// (the deserializer reached via `FUN_00ac4190` case 1 -> `FUN_00ac3fa0` ->
/// `FUN_00ac2260`) rather than guessing from Ghidra notes alone. This
/// corrected the earlier (wrong) assumption that 3 plain `Vec<u32>` arrays
/// were read back-to-back before `unknown2`. The real read order, confirmed
/// instruction-by-instruction, is:
///   1. u64 @ struct offset 0x18 (guid)
///   2. length-prefixed raw byte buffer @ 0x30 (name) via call 0x748020
///   3. u32 @ 0x28 (unknown1)
///   4. Vec<u32> @ 0x90 via FUN_00847d10 (confirmed: `u32 count` then
///      `count` raw u32s, loop body is a flat copy)
///   5. array @ 0xa0 via FUN_00abde60 (count-prefixed, but each element
///      goes through its own sub-deserializer call (FUN_00abba70 ->
///      FUN_00aba210) -- i.e. NOT a bare u32 per element. Sending count=0
///      is still safe/skippable regardless of element type.)
///   6. Vec<u32> @ 0xb8 via FUN_00abb370 (1st of 4 total calls to this
///      function -- confirmed `u32 count` + `count` raw u32s)
///   7. u32 @ 0x194 (unknown2) -- plain value, NOT an array
///   8. Vec<u32> @ 0xec via FUN_00abb370 (2nd call)
///   9. Vec<u32> @ 0x1cc via FUN_00abb370 (3rd call)
///  10. Vec<u32> @ 0xd4 via FUN_00abc7e0 -- disassembled this and every
///      remaining sub-structure below: all of them follow the exact same
///      `u32 count` + `count` raw-element loop shape as FUN_00abb370, just
///      at different addresses, so each is modeled here as a plain
///      `Vec<u32>` too. "Empty" for all of them is just a u32 0.
///  11. bool @ 0x1b0 (confirmed: `bool` impl writes exactly 1 byte, matches
///      the client's single-byte bounds-checked read here)
///  12. Vec<u32> @ 0x198 via FUN_00abce70
///  13. Vec<u32> @ 0x1b4 via FUN_00abcdb0
///  14. u32 @ 0x2c (unknown3) -- plain value
///  15. Vec<u32> @ 0x11c via FUN_00abb440
///  16. Vec<u32> @ struct offset 0x0 (the object's own base address) via
///      FUN_00abb4f0 -- unusual call site (`push edi; push esi` instead of
///      `lea reg,[edi+N]; push reg; push esi`), but same count-prefixed
///      shape; offset 0x0-0x17 is otherwise unused (guid starts at 0x18) so
///      this doesn't collide with any other field.
///  17. Vec<u32> @ 0x134 via FUN_00abb5a0
///  18. Vec<u32> @ 0x14c via FUN_00abb370 (4th and final call to this
///      function) -- NEW CANDIDATE, see TEST 4 below.
///  ...then 2 more sub-structures (FUN_00abdd30, FUN_00abb650) and a
///  trailing u64 @ 0x88 -- left unimplemented; the client's deserializer
///  zero-fills/truncates anything past the end of the buffer without
///  erroring (confirmed in the disassembly: every field read is
///  bounds-checked against the buffer end and defaults to 0/empty + sets a
///  "truncated" flag byte rather than panicking), so it's safe to stop the
///  buffer right after the field we're testing.
///
/// TEST 1 (field 6, the 1st FUN_00abb370 array): FAILED.
/// TEST 2 (field 8, the 2nd FUN_00abb370 array): FAILED.
/// TEST 3 (field 9, the 3rd FUN_00abb370 array): FAILED. All three failed
/// silently -- Gear tab stayed blank, no error logged, packet confirmed
/// sent. Also ruled out a guid-mismatch as the sole blocker (tried guid=0
/// and guid=real player).
///
/// TEST 4 (field 18, the 4th and last FUN_00abb370 array): FAILED. Exhausted
/// every plain `Vec<u32>`-shaped position in the packet.
///
/// TEST 5 (also populating `name` with the real player display name, in
/// addition to TEST 4's placement): FAILED.
///
/// TEST 6 (field 5 `unknown_array2` populated with real `ProfileGearEntry`
/// data): FAILED, same as every prior test -- packet sent, no errors, Gear
/// tab still blank.
///
/// ROOT CAUSE FOUND (disassembling the *caller* of this function,
/// `FUN_00ac3fa0`, around address `0xac404f`): the caller builds a cursor
/// `{data, len, current, end}` around our buffer and calls this deserializer
/// once. After it returns, the caller checks TWO things before treating the
/// response as valid: (1) the per-field "truncated" flag byte (the same one
/// set by every bounds-checked field read in this function, sitting at
/// cursor-object offset 0x10) must be UNSET, and (2) `end - current` must be
/// `<= 0`, i.e. the parser must have consumed every single byte we sent with
/// nothing left over. If either check fails, the caller jumps to a failure
/// path (sets an internal status to a "parse failed" sentinel) and the
/// response is effectively discarded -- which fully explains why TEST 1-6
/// all failed identically and silently regardless of which field we
/// changed: we were always deliberately truncating the buffer after
/// `gear_items`/`unknown_array2`, decades before the struct's real end, so
/// the truncation flag was *always* set and the whole `ProfileInfo` was
/// always being thrown away by the client before any tab (Gear or
/// otherwise) could render from it. "Safe to truncate" was true only in the
/// sense that the client wouldn't crash -- it silently discards the entire
/// packet instead.
///
/// This function's full extent was re-confirmed via objdump from
/// `0xac2260` to its `ret 0x10` at `0xac25a6`: there are exactly 21 fields
/// total, ending with fields 19-20 (two more `FUN_00abdd30`/`FUN_00abb650`
/// count-prefixed sub-structures, same shape as all the others) and field 21
/// (a final raw `u64` @ struct offset 0x88). After field 21 there are no
/// more wire reads -- only post-processing (locale-aware display name
/// formatting) and a debug log call.
///
/// TEST 7 (current): send the COMPLETE 21-field struct with no truncation
/// (added `unknown_array12`, `unknown_array13`, `trailing_unknown` below),
/// keeping the TEST 6 `ProfileGearEntry` population for field 5. This is the
/// first test where the buffer should be consumed exactly with no
/// truncation flag set, which per the root-cause finding above should be
/// required for the client to treat the response as valid at all.
#[derive(SerializePacket)]
pub struct SocialPacketProfileInfo {
    pub guid: u64,
    pub name: Vec<u8>,
    pub unknown1: u32,
    pub unknown_array1: Vec<u32>,
    pub unknown_array2: Vec<ProfileGearEntry>,
    pub unknown_array3: Vec<u32>,
    pub unknown2: u32,
    pub unknown_array4: Vec<u32>,
    pub unknown_array5: Vec<u32>,
    pub unknown_array6: Vec<u32>,
    pub unknown_bool: bool,
    pub unknown_array7: Vec<u32>,
    pub unknown_array8: Vec<u32>,
    pub unknown3: u32,
    pub unknown_array9: Vec<u32>,
    pub unknown_array10: Vec<u32>,
    pub unknown_array11: Vec<u32>,
    pub gear_items: Vec<u32>,
    pub unknown_array12: Vec<ProfileItemEntry>,
    pub unknown_array13: Vec<EquippedItemEntry>,
    pub trailing_unknown: u64,
}

impl GamePacket for SocialPacketProfileInfo {
    type Header = SocialOpCode;
    const HEADER: Self::Header = SocialOpCode::ProfileInfo;
}
