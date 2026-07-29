//! Per-player profile persistence.
//!
//! Each player's customizable state is stored as `profiles/<guid>.json` inside the
//! server's config directory.  On login the profile is loaded (or defaults are used
//! when no file exists yet).  The profile is re-saved whenever the player equips
//! an item, changes their appearance, collects something, or updates their favourites.

use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    path::Path,
};

use serde::{Deserialize, Serialize};

use crate::game_server::packets::player_update::CustomizationSlot;

use super::character::PlayerInventory;

/// Saveable per-player state.  Fields that are absent in an on-disk profile are
/// filled in from the test-data defaults at login.
#[derive(Default, Serialize, Deserialize)]
pub struct PlayerProfile {
    /// Display name shown on the nameplate.
    pub first_name: Option<String>,
    pub last_name: Option<String>,

    /// Equipped items: battle-class GUID → equipment slot (as u32) → item GUID.
    pub equipped: BTreeMap<u32, BTreeMap<u32, u32>>,

    /// Appearance: CustomizationSlot (as i32) → customization GUID.
    pub customizations: BTreeMap<i32, u32>,

    /// name_ids of collectibles this player has already picked up.
    pub collected_items: HashSet<u32>,

    /// Favourite emote quick_chat_ids, newest first (max 4).
    pub fav_emotes: Vec<i32>,

    /// item_guids purchased from the in-game store.
    pub purchased_items: HashSet<u32>,

    /// Current credit balance.  `None` means the profile predates credit
    /// tracking and the server will assign the default starting balance.
    #[serde(default)]
    pub credits: Option<u32>,
}

impl PlayerProfile {
    /// Snapshot in-memory player state into a profile ready to be flushed to disk.
    pub fn from_player(
        name: &crate::game_server::packets::Name,
        inventory: &PlayerInventory,
        customizations: &BTreeMap<CustomizationSlot, u32>,
        collected_items: &HashSet<u32>,
        fav_emotes: &VecDeque<i32>,
        purchased_items: &HashSet<u32>,
        credits: u32,
    ) -> Self {
        let equipped = inventory
            .battle_classes()
            .iter()
            .map(|(bc_guid, bc)| {
                let slots = bc
                    .items
                    .iter()
                    .map(|(slot, item_guid)| (*slot as u32, *item_guid))
                    .collect();
                (*bc_guid, slots)
            })
            .collect();

        let customizations_map = customizations
            .iter()
            .map(|(slot, guid)| (*slot as i32, *guid))
            .collect();

        PlayerProfile {
            first_name: Some(name.first_name.clone()),
            last_name: Some(name.last_name.clone()),
            equipped,
            customizations: customizations_map,
            collected_items: collected_items.clone(),
            fav_emotes: fav_emotes.iter().copied().collect(),
            purchased_items: purchased_items.clone(),
            credits: Some(credits),
        }
    }
}

/// Load a player's profile from `profiles_dir/<guid>.json`.
/// Returns all-defaults if the file is absent or unparseable.
pub fn load_profile(profiles_dir: &Path, guid: u32) -> PlayerProfile {
    let path = profiles_dir.join(format!("{guid}.json"));
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return PlayerProfile::default(),
    };
    serde_json::from_str(&text).unwrap_or_else(|err| {
        eprintln!("Warning: could not parse profile for player {guid}: {err}");
        PlayerProfile::default()
    })
}

/// Persist `profile` to `profiles_dir/<guid>.json`, creating the directory as needed.
/// Errors are printed but not propagated — a failed save is non-fatal.
pub fn save_profile(profiles_dir: &Path, guid: u32, profile: &PlayerProfile) {
    if let Err(err) = std::fs::create_dir_all(profiles_dir) {
        eprintln!("Warning: could not create profiles dir: {err}");
        return;
    }
    let path = profiles_dir.join(format!("{guid}.json"));
    match serde_json::to_string_pretty(profile) {
        Ok(text) => {
            if let Err(err) = std::fs::write(&path, text) {
                eprintln!("Warning: could not write profile for player {guid}: {err}");
            }
        }
        Err(err) => eprintln!("Warning: could not serialize profile for player {guid}: {err}"),
    }
}
