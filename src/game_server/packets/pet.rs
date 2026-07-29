use packet_serialize::SerializePacket;

use super::{GamePacket, OpCode};

#[derive(Copy, Clone, Debug)]
pub enum PetOpCode {
    /// Level-1 sub-opcode for the pet system: consumed by the per-opcode handler
    /// registered for OpCode 0x35, which routes to 0x8c14e0 (the pet sub-system
    /// dispatcher).  0x8c14e0 then reads a second 3-byte header [i16][i8] where
    /// the i8 is the level-2 sub-opcode (0x05 for PetInventory; see `PetInventory`).
    ///
    /// Previous analysis incorrectly assumed this was the only sub-opcode level.
    /// The correct chain: 0x0D → 0x8c14e0 → (i8=0x05) → 0x8c1f67 → 0x8c1020 → 0x8c00b0.
    Inventory = 0x0D,
}

impl SerializePacket for PetOpCode {
    fn serialize(&self, buffer: &mut Vec<u8>) {
        OpCode::Pet.serialize(buffer);
        (*self as u8).serialize(buffer);
    }
}

/// One item in the `BaseClient.PetInventory` DataSource.
///
/// Field mapping (from C++ bridge-registration analysis of CloneWars.exe):
///  instance_guid → "Item Guid"   ([item+0x11c] = pre_item_id, read before 0x78af80)
///  template_id   → "Item ID"     ([item+0x00], field 1)
///  item_type     → bridge router  ([item+0x08], field 3) — NOT icon_id.
///                  Controls which sub-DataSource the item is routed to:
///                    1  = BaseClient.PetInventory.HeadData    [esi+0x1328]
///                    3  = BaseClient.PetInventory.ChestData   [esi+0x1330]
///                    4  = BaseClient.PetInventory.FeetData    [esi+0x1334]
///                    5  = BaseClient.PetInventory.CollarData  [esi+0x132c]
///                  202  = BaseClient.PetInventory.ToyData     [esi+0x1338]
///                    0  = BaseClient.PetInventory (main)      [esi+0x133c]
///  name          → "Name"        ([item+0x34], string field 9)
///  tint_value    → [item+0x1C]   (field 13) = "Icon ID" in the DataSource.
///                                Lua (GenericItemSelectionData.lua, populateDataPetAttachments)
///                                reads GetData("Icon ID") and uses it as imageSetId for the
///                                icon graphic in the HudMenuBar_PetAttachments flyout.
pub struct PetInventoryItem {
    pub instance_guid: u32,
    pub template_id: u32,
    /// Sub-DataSource routing type.
    ///   0  = BaseClient.PetInventory (main)      [esi+0x133c]  ← USE THIS
    ///   1  = BaseClient.PetInventory.HeadData    [esi+0x1328]
    ///   3  = BaseClient.PetInventory.ChestData   [esi+0x1330]
    ///   4  = BaseClient.PetInventory.FeetData    [esi+0x1334]
    ///   5  = BaseClient.PetInventory.CollarData  [esi+0x132c]
    /// 202  = BaseClient.PetInventory.ToyData     [esi+0x1338]
    ///
    /// Lua's populateDataPetAttachments (GenericItemSelectionData.lua) reads ONLY from
    /// "BaseClient.PetInventory" (the main DataSource), so all attachment items must use
    /// item_type=0.  The slot-specific sub-DataSources (HeadData etc.) are never read by Lua.
    pub item_type: u32,
    pub name: String,
    pub tint_value: u32,
}

/// Populates `BaseClient.PetInventory` DataSource so the gear-icon flyout
/// in `ActivePetWindow` shows the player's owned attachment items.
///
/// ## Wire format
///
/// The packet has **two sub-opcode levels**:
///
/// ```text
/// [OpCode 0x35 u16]          ← consumed by SOE outer dispatcher
/// [level-1 sub-op 0x0D u8]  ← PetOpCode::Inventory; consumed by per-opcode handler
///                              which routes to 0x8c14e0 (the pet sub-system dispatcher)
/// [i16 = 0u16]               \
/// [i8  = 0x05u8]             /  3-byte header; 0x8c14e0 reads these:
///                              i16 is ignored; i8 is the level-2 sub-opcode.
///                              Sub-opcode 5 → jump table → 0x8c1f67 → 0x8c1020 → 0x8c00b0.
/// [item_count u32]
/// [per-item fields × N]      ← 0x8bfe40 / 0x78af80 reads each item
/// [companion_guid_lo u32]    → [out+0x2C]
/// [companion_guid_hi u32]    → [out+0x30]
/// [pet_item_id u32]          → [out+0x34]
/// ```
///
/// 0x8c00b0 itself re-reads the 3-byte header (calls 0x8ba160 first) to skip
/// it before delegating to 0x8bfe40 for the item list.
pub struct PetInventory {
    pub items: Vec<PetInventoryItem>,
    /// Low 32 bits of the companion NPC GUID (or 0 to use old behaviour).
    pub companion_guid_lo: u32,
    /// High 32 bits of the companion NPC GUID (or 0).
    pub companion_guid_hi: u32,
    /// The pet's item_guid (e.g. 917 for B3-T4), or 0.
    pub pet_item_id: u32,
}

impl SerializePacket for PetInventory {
    fn serialize(&self, buffer: &mut Vec<u8>) {
        // 3-byte level-2 header consumed by 0x8c14e0 (and re-skipped by 0x8c00b0):
        //   i16 = 0   (ignored by dispatcher)
        //   i8  = 5   ← level-2 sub-opcode: routes to handler 0x8c1f67 in dispatch
        //               table at 0x8c36f4/0x8c36a0; i8=0 causes immediate exit via
        //               `add eax,-1; ja bounds` check. MUST be 0x05.
        0u16.serialize(buffer);
        0x05u8.serialize(buffer);

        // item_count (u32)
        (self.items.len() as u32).serialize(buffer);

        for item in &self.items {
            // pre_item_id → [item+0x11c] = "Item Guid" (read by 0x8bfe40 before 0x78af80)
            item.instance_guid.serialize(buffer);

            // --- 0x78af80 per-item reader fields (in packet order) ---
            // 1.  u32  → [item+0x00] = "Item ID"
            item.template_id.serialize(buffer);
            // 2.  u8   → [item+0x04] bool — possible "equipped/unavailable" flag;
            //            try false (0) so items appear as available-to-select.
            0u8.serialize(buffer);
            // 3.  u32  → [item+0x08] = item type (bridge router: 1=Head, 3=Chest,
            //                         4=Feet, 5=Collar, 202=Toy, 0=main PetInventory)
            item.item_type.serialize(buffer);
            // 4-7. f32 NaN-checked → [item+0x20..0x2C] (4 floats)
            (0.0f32).serialize(buffer);
            (0.0f32).serialize(buffer);
            (0.0f32).serialize(buffer);
            (0.0f32).serialize(buffer);
            // 8.  u8   → [item+0x30] bool — set true for the same reason.
            1u8.serialize(buffer);
            // 9.  Name string via 0x774DD0: u32 char_count + char_count×u32 codepoints
            let chars: Vec<u32> = item.name.chars().map(|c| c as u32).collect();
            (chars.len() as u32).serialize(buffer);
            for cp in &chars {
                cp.serialize(buffer);
            }
            // 10. [item+0xBC] array via 0x75EA20: u32 count + count×u32 (empty)
            0u32.serialize(buffer);
            // 11. [item+0xE4] array via 0x774600: u32 count + count×u32 (empty)
            0u32.serialize(buffer);
            // 12. blob [item+0x0C] via 0x748020: u32 len + len bytes (empty)
            0u32.serialize(buffer);
            // 13. u32  → [item+0x1C] = "Icon ID" (DataSource column read by Lua as imageSetId)
            item.tint_value.serialize(buffer);
            // 14. blob [item+0x100] via 0x748020: u32 len + len bytes (empty)
            0u32.serialize(buffer);
            // 15. u32  → [item+0xFC]
            0u32.serialize(buffer);
            // 16. u8   → [item+0x110] bool
            0u8.serialize(buffer);
            // 17. u32  → [item+0x114]
            0u32.serialize(buffer);
            // 18. 4× u32 → [item+0x88..0x94]
            for _ in 0..4 {
                0u32.serialize(buffer);
            }
            // 19. 8× u32 → [item+0x98..0xB4]
            for _ in 0..8 {
                0u32.serialize(buffer);
            }
        }

        // Trailing 3 u32s read by 0x8c00b0 after the item loop
        // [out+0x2C] = companion_guid_lo (low 32 bits of companion NPC GUID)
        // [out+0x30] = companion_guid_hi (high 32 bits of companion NPC GUID)
        // [out+0x34] = pet_item_id (the companion's item_guid, e.g. 917)
        self.companion_guid_lo.serialize(buffer);
        self.companion_guid_hi.serialize(buffer);
        self.pet_item_id.serialize(buffer);
    }
}

impl GamePacket for PetInventory {
    type Header = PetOpCode;
    const HEADER: Self::Header = PetOpCode::Inventory;
}
