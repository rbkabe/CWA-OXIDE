use std::{
    collections::{BTreeMap, HashSet},
    fs::File,
    path::Path,
};

use serde::Deserialize;

use crate::{
    game_server::{
        handlers::{item::ItemConfig, store::{CostEntry, ItemCostMap}},
        packets::{
            quick_chat::QuickChatDefinition,
            reference_data::{
                CategoryDefinitions, ItemClassDefinition, ItemClassDefinitions,
                ItemGroupDefinition, ItemGroupItem,
            },
        },
    },
    ConfigError,
};

pub fn load_item_classes(config_dir: &Path) -> Result<ItemClassDefinitions, ConfigError> {
    let mut file = File::open(config_dir.join("item_classes.yaml"))?;
    let definitions: Vec<ItemClassDefinition> = serde_yaml::from_reader(&mut file)?;
    Ok(ItemClassDefinitions {
        definitions: definitions
            .into_iter()
            .map(|definition| (definition.guid, definition))
            .collect(),
    })
}

pub fn load_categories(config_dir: &Path) -> Result<CategoryDefinitions, ConfigError> {
    let mut file = File::open(config_dir.join("item_categories.yaml"))?;
    Ok(serde_yaml::from_reader(&mut file)?)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemGroupConfig {
    pub guid: i32,
    #[serde(default)]
    pub name_id: u32,
    #[serde(default)]
    pub description_id: u32,
    #[serde(default)]
    pub sort_order: u32,
    #[serde(default)]
    pub icon_set_id: u32,
    #[serde(default)]
    pub category: u32,
    #[serde(default)]
    pub page: u32,
    #[serde(default)]
    pub preview_model_id: u32,
    #[serde(default)]
    pub preview_animation_id: i32,
    #[serde(default)]
    pub is_new: bool,
    #[serde(default)]
    pub members_only: bool,
    #[serde(default)]
    pub for_sale: bool,
    /// Price for every item in this group when set > 0.  Overrides the
    /// per-item `cost` field from the individual item YAML files, making it
    /// easy to price a whole outfit/set from one place in item_groups.yaml.
    /// When 0 (the default), the per-item cost is used unchanged.
    #[serde(default)]
    pub cost: u32,
    /// Optional display name sent as a literal string in the packet.
    /// Use this for custom items whose name_id can't be resolved by the client
    /// (e.g. large IDs > 65535 that overflow the client's u16 locale lookup).
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub items: Vec<ItemGroupItem>,
}

impl From<ItemGroupConfig> for ItemGroupDefinition {
    fn from(value: ItemGroupConfig) -> Self {
        ItemGroupDefinition {
            guid: value.guid,
            unknown2: 0,
            name_id: value.name_id,
            description_id: value.description_id,
            sort_order: value.sort_order,
            icon_set_id: value.icon_set_id,
            category: value.category,
            page: value.page,
            preview_model_id: value.preview_model_id,
            preview_animation_id: value.preview_animation_id,
            is_new: value.is_new,
            unknown12: 0,
            unknown13: 0,
            unknown14: 0,
            unknown16: value.display_name,
            members_only: value.members_only,
            items: value.items,
        }
    }
}

#[derive(Deserialize)]
pub struct QuickChatConfig {
    #[serde(default)]
    pub comment: String,
    pub id: i32,
    pub parent_id: i32,
    pub menu_text: i32,
    pub menu_icon_id: i32,
    #[serde(default)]
    pub animation_id: i32,
    #[serde(default)]
    pub item_id: i32,
}

pub fn load_quick_chats(config_dir: &Path) -> Result<Vec<QuickChatDefinition>, ConfigError> {
    let mut file = File::open(config_dir.join("quick_chats.yaml"))?;
    let configs: Vec<QuickChatConfig> = serde_yaml::from_reader(&mut file)?;
    Ok(configs
        .into_iter()
        .map(|c| QuickChatDefinition {
            id: c.id,
            id2: c.id,
            menu_text: c.menu_text,
            chat_text: 0,
            animation_id: c.animation_id,
            unknown1: 0,
            admin_only: 0,
            menu_icon_id: c.menu_icon_id,
            item_id: c.item_id,
            parent_id: c.parent_id,
            unknown2: 0,
        })
        .collect())
}

/// An unnamed multi-item group that would show up as a blank "item set"
/// listing: one with a store category.  Unnamed groups with category 0 are
/// still sent: the store's single-item view only lists items whose group the
/// client knows about (e.g. our hidden groups 10100+ for standalone items).
fn is_hidden_store_group(group: &ItemGroupConfig) -> bool {
    group.category != 0
}

pub fn load_item_groups(
    config_dir: &Path,
    items: &BTreeMap<u32, ItemConfig>,
    costs: &mut ItemCostMap,
) -> Result<Vec<ItemGroupDefinition>, ConfigError> {
    let mut file = File::open(config_dir.join("item_groups.yaml"))?;
    let groups: Vec<ItemGroupConfig> = serde_yaml::from_reader(&mut file)?;

    for group in groups.iter() {
        for item in group.items.iter() {
            if !items.contains_key(&item.guid) {
                return Err(ConfigError::ConstraintViolated(format!(
                    "Item group {} contains unknown item {}",
                    group.guid, item.guid
                )));
            }
        }
    }

    let items_for_sale: HashSet<u32> = groups
        .iter()
        .filter(|group| group.for_sale)
        .flat_map(|group| group.items.iter().map(|item| item.guid))
        .collect();

    // Apply group-level cost override: when a for_sale group has cost > 0,
    // that price wins over the individual item's cost field.
    for group in groups.iter() {
        if group.for_sale && group.cost > 0 {
            for item in group.items.iter() {
                if let Some(entry) = costs.get_mut(&item.guid) {
                    entry.base = group.cost;
                    entry.members = group.cost;
                }
            }
        }
    }

    costs.retain(|item_guid, _| items_for_sale.contains(item_guid));

    // Add group-level costs for multi-item bundles so the client sees a price for the
    // whole set and BuyItem with the group guid is handled server-side.
    // Unnamed groups (name_id 0, no display_name) only exist to put their items
    // up for sale individually; giving them a bundle price makes them show up in
    // the store as blank "item set" listings, so they get no group cost.
    for group in groups
        .iter()
        .filter(|g| g.for_sale && g.items.len() > 1)
        .filter(|g| g.name_id != 0 || !g.display_name.is_empty())
    {
        let group_guid = group.guid as u32;
        let total: u32 = group
            .items
            .iter()
            .filter_map(|item| costs.get(&item.guid))
            .map(|e| e.base)
            .sum();
        if total > 0 {
            costs.entry(group_guid).or_insert(CostEntry {
                base: total,
                members: total,
            });
        }
    }

    // Don't send our unnamed store groups to the client: it lists every store
    // group definition it receives as an "item set" (pricing it itself from the
    // member items), and these would show as blank listings.  They have already
    // done their job above — putting their items up for sale.
    //
    // The original game's unnamed, uncategorised groups must still be sent: the
    // client binds them as picker lists, e.g. 3/4/5 = ItemGroup.LightsaberHilts/
    // ColorGems/Shapes and 8/9/149 their offhand versions (Lightsaber
    // Customization), 122-127/140 the Character Customization lists.
    Ok(groups
        .into_iter()
        .filter(|g| !(g.items.len() > 1 && g.name_id == 0 && g.display_name.is_empty()) || !is_hidden_store_group(g))
        .map(|group| group.into())
        .collect())
}
