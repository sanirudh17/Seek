//! Favorites and user groups in the sidebar (3.4). Pure data and operations,
//! independent of GPUI. Groups only arrange shortcuts: no operation here ever
//! touches a file, and deleting a group returns its items to Favorites.
use crate::settings::Search;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const MAX_GROUPS: usize = 32;
pub const MAX_ITEMS: usize = 64;
pub const MAX_NAME: usize = 64;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Item {
    Folder {
        path: PathBuf,
        #[serde(default)]
        label: Option<String>,
    },
    /// A saved search; it runs its query whenever it is opened.
    Search { search: Search },
}
impl Item {
    pub fn folder(path: PathBuf, label: Option<String>) -> Self {
        Self::Folder { path, label }
    }
    pub fn label(&self) -> String {
        match self {
            Self::Folder { path, label } => label.clone().unwrap_or_else(|| {
                path.file_name()
                    .unwrap_or(path.as_os_str())
                    .to_string_lossy()
                    .trim_start_matches(r"\\?\")
                    .to_owned()
            }),
            Self::Search { search } => search.query.clone(),
        }
    }
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Folder { path, .. } => Some(path),
            Self::Search { .. } => None,
        }
    }
    fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Folder { path: a, .. }, Self::Folder { path: b, .. }) => same_path(a, b),
            (Self::Search { search: a }, Self::Search { search: b }) => a == b,
            _ => false,
        }
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    a.as_os_str()
        .to_string_lossy()
        .trim_end_matches('\\')
        .eq_ignore_ascii_case(b.as_os_str().to_string_lossy().trim_end_matches('\\'))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default)]
    pub items: Vec<Item>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Favorites,
    Group(u64),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    /// Ungrouped favorites, in order. Only meaningful once `arranged`.
    pub favorites: Vec<Item>,
    pub groups: Vec<Group>,
    pub next_id: u64,
    /// False until the user arranges anything; Favorites then follow the
    /// known Windows folders exactly.
    pub arranged: bool,
}

impl Layout {
    /// The layout to show and edit: known folders the user has not placed
    /// anywhere are appended to Favorites, so nothing silently disappears.
    pub fn resolve(&self, known: &[(String, PathBuf)]) -> Self {
        let mut layout = self.clone();
        if !layout.arranged {
            layout.favorites = known
                .iter()
                .map(|(name, path)| Item::folder(path.clone(), Some(name.clone())))
                .collect();
        } else {
            for (name, path) in known {
                let item = Item::folder(path.clone(), Some(name.clone()));
                if !layout.contains(&item) {
                    layout.favorites.push(item);
                }
            }
        }
        layout.sanitize();
        layout
    }

    /// Bound sizes and repair identities after reading untrusted settings.
    pub fn sanitize(&mut self) {
        self.groups.truncate(MAX_GROUPS);
        self.favorites.truncate(MAX_ITEMS * 4);
        let mut seen = std::collections::BTreeSet::new();
        for group in &mut self.groups {
            group.name = clean_name(&group.name).unwrap_or_else(|| "Group".into());
            group.items.truncate(MAX_ITEMS);
            if group.id == 0 || !seen.insert(group.id) {
                group.id = 0;
            }
        }
        let mut next = self
            .next_id
            .max(self.groups.iter().map(|g| g.id).max().unwrap_or(0) + 1);
        for group in &mut self.groups {
            if group.id == 0 {
                group.id = next;
                next += 1;
            }
        }
        self.next_id = next.max(1);
    }

    fn contains(&self, item: &Item) -> bool {
        self.favorites.iter().any(|i| i.same(item))
            || self
                .groups
                .iter()
                .any(|g| g.items.iter().any(|i| i.same(item)))
    }

    fn items_mut(&mut self, place: Place) -> Option<&mut Vec<Item>> {
        match place {
            Place::Favorites => Some(&mut self.favorites),
            Place::Group(id) => self
                .groups
                .iter_mut()
                .find(|g| g.id == id)
                .map(|g| &mut g.items),
        }
    }

    pub fn items(&self, place: Place) -> Option<&Vec<Item>> {
        match place {
            Place::Favorites => Some(&self.favorites),
            Place::Group(id) => self.groups.iter().find(|g| g.id == id).map(|g| &g.items),
        }
    }

    pub fn create_group(&mut self, name: &str) -> Result<u64, String> {
        if self.groups.len() >= MAX_GROUPS {
            return Err(format!("The sidebar holds up to {MAX_GROUPS} groups."));
        }
        let name = clean_name(name).ok_or("Enter a group name.")?;
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        self.groups.push(Group {
            id,
            name,
            collapsed: false,
            items: vec![],
        });
        self.arranged = true;
        Ok(id)
    }

    pub fn rename_group(&mut self, id: u64, name: &str) -> Result<(), String> {
        let name = clean_name(name).ok_or("Enter a group name.")?;
        let group = self
            .groups
            .iter_mut()
            .find(|g| g.id == id)
            .ok_or("That group no longer exists.")?;
        group.name = name;
        Ok(())
    }

    /// Delete a group; its items go back to the end of Favorites.
    pub fn delete_group(&mut self, id: u64) -> Result<(), String> {
        let index = self
            .groups
            .iter()
            .position(|g| g.id == id)
            .ok_or("That group no longer exists.")?;
        let group = self.groups.remove(index);
        for item in group.items {
            if !self.favorites.iter().any(|i| i.same(&item)) {
                self.favorites.push(item);
            }
        }
        self.arranged = true;
        Ok(())
    }

    pub fn toggle(&mut self, id: u64) -> Option<bool> {
        let group = self.groups.iter_mut().find(|g| g.id == id)?;
        group.collapsed = !group.collapsed;
        self.arranged = true;
        Some(group.collapsed)
    }

    /// Insert an item at slot `index` of `place`. An item already in the
    /// sidebar moves there instead of being duplicated.
    pub fn place_item(
        &mut self,
        item: Item,
        place: Place,
        index: Option<usize>,
    ) -> Result<(), String> {
        let target_len = self
            .items(place)
            .ok_or("That group no longer exists.")?
            .len();
        let existing = self.find(&item);
        if existing.is_none() && target_len >= MAX_ITEMS && place != Place::Favorites {
            return Err(format!("A group holds up to {MAX_ITEMS} items."));
        }
        let mut slot = index.unwrap_or(target_len).min(target_len);
        let item = if let Some((from, position)) = existing {
            if from == place && position < slot {
                slot -= 1;
            }
            self.items_mut(from).unwrap().remove(position)
        } else {
            item
        };
        let target = self.items_mut(place).unwrap();
        let slot = slot.min(target.len());
        target.insert(slot, item);
        self.arranged = true;
        Ok(())
    }

    pub fn remove_item(&mut self, place: Place, index: usize) -> Option<Item> {
        let items = self.items_mut(place)?;
        (index < items.len())
            .then(|| items.remove(index))
            .inspect(|_| self.arranged = true)
    }

    fn find(&self, item: &Item) -> Option<(Place, usize)> {
        if let Some(i) = self.favorites.iter().position(|i| i.same(item)) {
            return Some((Place::Favorites, i));
        }
        self.groups.iter().find_map(|g| {
            g.items
                .iter()
                .position(|i| i.same(item))
                .map(|i| (Place::Group(g.id), i))
        })
    }

    /// Move a group to insertion slot `slot` among the groups.
    pub fn move_group(&mut self, id: u64, slot: usize) -> Result<(), String> {
        let from = self
            .groups
            .iter()
            .position(|g| g.id == id)
            .ok_or("That group no longer exists.")?;
        let slot = slot.min(self.groups.len());
        let group = self.groups.remove(from);
        let to = if from < slot { slot - 1 } else { slot };
        self.groups.insert(to.min(self.groups.len()), group);
        self.arranged = true;
        Ok(())
    }

    /// Items in on-screen order (Favorites, then each group), for Alt+1-9.
    pub fn numbered(&self) -> Vec<&Item> {
        self.favorites
            .iter()
            .chain(self.groups.iter().flat_map(|g| g.items.iter()))
            .collect()
    }
}

fn clean_name(name: &str) -> Option<String> {
    let name: String = name
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(MAX_NAME)
        .collect();
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn known() -> Vec<(String, PathBuf)> {
        vec![
            ("Downloads".into(), PathBuf::from(r"C:\Users\a\Downloads")),
            ("Documents".into(), PathBuf::from(r"C:\Users\a\Documents")),
        ]
    }
    fn search(query: &str) -> Search {
        Search {
            query: query.into(),
            scope: None,
            content: false,
        }
    }

    #[test]
    fn unarranged_favorites_follow_known_folders() {
        let layout = Layout::default().resolve(&known());
        assert_eq!(layout.favorites.len(), 2);
        assert_eq!(layout.favorites[0].label(), "Downloads");
        assert!(layout.groups.is_empty());
    }

    #[test]
    fn groups_create_rename_toggle_and_delete_without_losing_items() {
        let mut layout = Layout::default().resolve(&known());
        let id = layout.create_group("  Work  ").unwrap();
        assert_eq!(layout.groups[0].name, "Work");
        assert!(layout.create_group("   ").is_err());
        layout
            .place_item(layout.favorites[1].clone(), Place::Group(id), None)
            .unwrap();
        layout
            .place_item(
                Item::Search {
                    search: search("ext:pdf"),
                },
                Place::Group(id),
                None,
            )
            .unwrap();
        assert_eq!(layout.favorites.len(), 1);
        assert_eq!(layout.groups[0].items.len(), 2);
        layout.rename_group(id, "Projects").unwrap();
        assert_eq!(layout.toggle(id), Some(true));
        assert!(layout.groups[0].collapsed);
        layout.delete_group(id).unwrap();
        assert!(layout.groups.is_empty());
        assert_eq!(layout.favorites.len(), 3);
        assert_eq!(layout.favorites[1].label(), "Documents");
        // A known folder moved into a group is not re-added to Favorites.
        let mut layout = Layout::default().resolve(&known());
        let id = layout.create_group("A").unwrap();
        layout
            .place_item(layout.favorites[0].clone(), Place::Group(id), None)
            .unwrap();
        let again = layout.resolve(&known());
        assert_eq!(again.favorites.len(), 1);
        assert_eq!(again.groups[0].items.len(), 1);
    }

    #[test]
    fn placing_moves_instead_of_duplicating_and_uses_insertion_slots() {
        let mut layout = Layout::default().resolve(&known());
        let extra = Item::folder(PathBuf::from(r"D:\Media"), None);
        layout
            .place_item(extra.clone(), Place::Favorites, Some(0))
            .unwrap();
        assert_eq!(layout.favorites[0].label(), "Media");
        // Same folder, different case and trailing slash: moves.
        layout
            .place_item(
                Item::folder(PathBuf::from(r"d:\media\"), None),
                Place::Favorites,
                None,
            )
            .unwrap();
        assert_eq!(layout.favorites.len(), 3);
        assert_eq!(layout.favorites[2].label(), "Media");
        // Slot is counted before removal.
        layout.place_item(extra, Place::Favorites, Some(1)).unwrap();
        assert_eq!(layout.favorites[1].label(), "Media");
    }

    #[test]
    fn groups_reorder_and_numbering_follows_screen_order() {
        let mut layout = Layout::default().resolve(&known());
        let a = layout.create_group("A").unwrap();
        let b = layout.create_group("B").unwrap();
        layout
            .place_item(
                Item::folder(PathBuf::from(r"D:\x"), None),
                Place::Group(b),
                None,
            )
            .unwrap();
        layout
            .place_item(
                Item::folder(PathBuf::from(r"D:\y"), None),
                Place::Group(a),
                None,
            )
            .unwrap();
        layout.move_group(b, 0).unwrap();
        assert_eq!(layout.groups[0].id, b);
        let labels: Vec<_> = layout.numbered().iter().map(|i| i.label()).collect();
        assert_eq!(labels, ["Downloads", "Documents", "x", "y"]);
        assert!(layout.move_group(999, 0).is_err());
    }

    #[test]
    fn hostile_settings_are_bounded_and_identities_repaired() {
        let mut layout = Layout {
            groups: (0..100)
                .map(|_| Group {
                    id: 7,
                    name: "\u{7}".repeat(5),
                    collapsed: false,
                    items: vec![Item::folder(PathBuf::from(r"C:\x"), None); 200],
                })
                .collect(),
            ..Default::default()
        };
        layout.sanitize();
        assert_eq!(layout.groups.len(), MAX_GROUPS);
        let ids: std::collections::BTreeSet<_> = layout.groups.iter().map(|g| g.id).collect();
        assert_eq!(ids.len(), MAX_GROUPS);
        assert!(
            layout
                .groups
                .iter()
                .all(|g| g.items.len() <= MAX_ITEMS && g.name == "Group")
        );
        assert!(layout.next_id > *ids.iter().max().unwrap());
    }
}
