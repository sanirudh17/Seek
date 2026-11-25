//! Versioned navigation/layout state, independent of GPUI and file operations.
//! No operation or text-edit draft can be reconstructed from a session.
use crate::settings::View;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::PathBuf};

pub const MAX_PANES: usize = 8;
pub const MAX_TABS: usize = 32;
pub const VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PaneId(pub u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TabId(pub u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SplitId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    /// First pane on the left, second on the right.
    Horizontal,
    /// First pane above the second.
    Vertical,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Node {
    Pane {
        id: PaneId,
    },
    Split {
        id: SplitId,
        axis: Axis,
        /// Basis points, to avoid non-finite persisted ratios.
        fraction: u16,
        first: Box<Node>,
        second: Box<Node>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Visit {
    pub query: String,
    #[serde(with = "optional_path")]
    pub scope: Option<PathBuf>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tab {
    pub id: TabId,
    pub query: String,
    #[serde(with = "optional_path")]
    pub scope: Option<PathBuf>,
    pub view: View,
    pub content: bool,
    pub largest: Option<bool>,
    pub normal: Option<Normal>,
    pub sort: Option<(usize, bool)>,
    pub history: Vec<Visit>,
    pub history_position: usize,
    pub list_offset: [f32; 2],
    pub grid_offset: [f32; 2],
    #[serde(with = "optional_path")]
    pub selected: Option<PathBuf>,
    #[serde(with = "paths")]
    pub expanded: Vec<PathBuf>,
    /// Column-view widths in pixels, left to right.
    #[serde(default)]
    pub column_widths: Vec<u16>,
    /// Related folders linked to this tab (3.6).
    #[serde(default)]
    pub related: Vec<RelatedFolder>,
    #[serde(default)]
    pub related_visible: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RelatedFolder {
    #[serde(with = "optional_path")]
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub label: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Normal {
    pub query: String,
    pub sort: Option<(usize, bool)>,
    pub view: View,
}
impl Tab {
    pub fn initial(id: TabId, view: View) -> Self {
        Self {
            id,
            query: String::new(),
            scope: None,
            view,
            content: false,
            largest: None,
            normal: None,
            sort: None,
            history: vec![Visit {
                query: String::new(),
                scope: None,
            }],
            history_position: 0,
            list_offset: [0.; 2],
            grid_offset: [0.; 2],
            selected: None,
            expanded: vec![],
            column_widths: vec![],
            related: vec![],
            related_visible: false,
        }
    }
    fn valid(&self) -> bool {
        self.query.len() <= 4096
            && valid_path(&self.scope)
            && valid_path(&self.selected)
            && self.history.len() <= 128
            && !self.history.is_empty()
            && self.history_position < self.history.len()
            && self
                .history
                .iter()
                .all(|visit| visit.query.len() <= 4096 && valid_path(&visit.scope))
            && self
                .list_offset
                .iter()
                .chain(&self.grid_offset)
                .all(|value| value.is_finite() && (-100_000_000. ..=0.).contains(value))
            && self.sort.is_none_or(|(column, _)| column < 5)
            && self.normal.as_ref().is_none_or(|normal| {
                normal.query.len() <= 4096 && normal.sort.is_none_or(|(column, _)| column < 5)
            })
            && self.expanded.len() <= 256
            && self
                .expanded
                .iter()
                .all(|path| valid_path(&Some(path.clone())))
            && !(self.content && self.largest.is_some())
            && self.column_widths.len() <= 16
            && self
                .column_widths
                .iter()
                .all(|width| (100..=1000).contains(width))
            && self.related.len() <= 16
            && self.related.iter().all(|chip| {
                chip.path.is_some()
                    && valid_path(&chip.path)
                    && chip
                        .label
                        .as_ref()
                        .is_none_or(|label| label.chars().count() <= 64)
            })
    }
}
fn valid_path(path: &Option<PathBuf>) -> bool {
    use std::os::windows::ffi::OsStrExt;
    path.as_ref().is_none_or(|path| {
        let units: Vec<_> = path.as_os_str().encode_wide().collect();
        path.is_absolute() && !units.is_empty() && units.len() <= 32767 && !units.contains(&0)
    })
}

// JSON strings cannot represent unpaired Windows UTF-16 surrogates. Store paths
// as code units so restoring a session never changes a filename.
mod optional_path {
    use super::*;
    use std::{
        ffi::OsString,
        os::windows::ffi::{OsStrExt, OsStringExt},
    };
    pub fn serialize<S: serde::Serializer>(
        path: &Option<PathBuf>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        path.as_ref()
            .map(|path| path.as_os_str().encode_wide().collect::<Vec<_>>())
            .serialize(s)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        d: D,
    ) -> Result<Option<PathBuf>, D::Error> {
        Ok(Option::<Vec<u16>>::deserialize(d)?.map(|units| OsString::from_wide(&units).into()))
    }
}
mod paths {
    use super::*;
    use std::{
        ffi::OsString,
        os::windows::ffi::{OsStrExt, OsStringExt},
    };
    pub fn serialize<S: serde::Serializer>(paths: &[PathBuf], s: S) -> Result<S::Ok, S::Error> {
        paths
            .iter()
            .map(|path| path.as_os_str().encode_wide().collect::<Vec<_>>())
            .collect::<Vec<_>>()
            .serialize(s)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<PathBuf>, D::Error> {
        Ok(Vec::<Vec<u16>>::deserialize(d)?
            .into_iter()
            .map(|units| OsString::from_wide(&units).into())
            .collect())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pane {
    pub id: PaneId,
    pub active_tab: TabId,
    pub tabs: Vec<Tab>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub version: u32,
    pub active: PaneId,
    pub root: Node,
    pub panes: Vec<Pane>,
    pub next_id: u64,
    /// Document panes (3.5): layout leaves that show one file.
    #[serde(default)]
    pub documents: Vec<DocumentPane>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentPane {
    pub id: PaneId,
    #[serde(with = "optional_path")]
    pub path: Option<PathBuf>,
}
impl Session {
    pub fn initial(view: View) -> Self {
        Self {
            version: VERSION,
            active: PaneId(1),
            root: Node::Pane { id: PaneId(1) },
            panes: vec![Pane {
                id: PaneId(1),
                active_tab: TabId(2),
                tabs: vec![Tab::initial(TabId(2), view)],
            }],
            next_id: 3,
            documents: vec![],
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.version != VERSION {
            return Err("Unsupported session version".into());
        }
        if self.panes.is_empty() || self.panes.len() + self.documents.len() > MAX_PANES {
            return Err("Invalid pane count".into());
        }
        let mut ids = BTreeSet::new();
        let mut leaves = BTreeSet::new();
        self.root.validate(&mut ids, &mut leaves, 0)?;
        let mut panes = BTreeSet::new();
        for pane in &self.panes {
            if !panes.insert(pane.id)
                || !leaves.contains(&pane.id)
                || pane.tabs.is_empty()
                || pane.tabs.len() > MAX_TABS
                || !pane.tabs.iter().any(|tab| tab.id == pane.active_tab)
            {
                return Err("Invalid pane or active tab".into());
            }
            for tab in &pane.tabs {
                if !ids.insert(tab.id.0) || !tab.valid() {
                    return Err("Invalid or duplicate tab".into());
                }
            }
        }
        // The focused pane is always a browser pane, never a document.
        let browsers = panes.clone();
        for document in &self.documents {
            if !panes.insert(document.id) || document.path.is_none() || !valid_path(&document.path)
            {
                return Err("Invalid document pane".into());
            }
        }
        if panes != leaves
            || !browsers.contains(&self.active)
            || ids.contains(&0)
            || ids.last().is_none_or(|id| *id >= self.next_id)
            || self.next_id == u64::MAX
        {
            return Err("Invalid session identities".into());
        }
        Ok(())
    }
    pub fn pane(&self, id: PaneId) -> Option<&Pane> {
        self.panes.iter().find(|pane| pane.id == id)
    }
    fn allocate(&mut self) -> Result<u64, String> {
        let id = self.next_id;
        self.next_id = id
            .checked_add(1)
            .filter(|id| *id < u64::MAX)
            .ok_or("Session identity limit reached")?;
        Ok(id)
    }
    /// Work on a bounded copy and commit only after full validation. Failed UI
    /// transfers never detach a tab or leave a partially changed tree.
    fn change<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T, String>) -> Result<T, String> {
        self.validate()?;
        let mut next = self.clone();
        let result = f(&mut next)?;
        next.validate()?;
        *self = next;
        Ok(result)
    }
    pub fn split(&mut self, pane: PaneId, axis: Axis) -> Result<PaneId, String> {
        self.change(|next| {
            if next.panes.len() + next.documents.len() == MAX_PANES {
                return Err("Pane limit reached".into());
            }
            let source = next.pane(pane).ok_or("Pane no longer exists")?;
            let mut tab = source
                .tabs
                .iter()
                .find(|tab| tab.id == source.active_tab)
                .unwrap()
                .clone();
            let id = PaneId(next.allocate()?);
            tab.id = TabId(next.allocate()?);
            let split = SplitId(next.allocate()?);
            next.root.replace(
                pane,
                Node::Split {
                    id: split,
                    axis,
                    fraction: 5000,
                    first: Box::new(Node::Pane { id: pane }),
                    second: Box::new(Node::Pane { id }),
                },
            );
            next.panes.push(Pane {
                id,
                active_tab: tab.id,
                tabs: vec![tab],
            });
            next.active = id;
            Ok(id)
        })
    }
    pub fn resize(&mut self, split: SplitId, fraction: u16) -> Result<(), String> {
        self.change(|next| {
            if !(1000..=9000).contains(&fraction) {
                return Err("Divider fraction is outside its bounds".into());
            }
            if !next.root.resize(split, fraction) {
                return Err("Divider no longer exists".into());
            }
            Ok(())
        })
    }
    /// Preserve tabs when closing a pane; a full destination refuses closure.
    pub fn close(&mut self, pane: PaneId, destination: PaneId) -> Result<(), String> {
        self.change(|next| {
            if pane == destination {
                return Err("Choose another pane".into());
            }
            let source = next.pane(pane).ok_or("Pane no longer exists")?.clone();
            let target = next
                .panes
                .iter_mut()
                .find(|pane| pane.id == destination)
                .ok_or("Destination pane no longer exists")?;
            if target.tabs.len() + source.tabs.len() > MAX_TABS {
                return Err("Destination tab limit reached".into());
            }
            target.tabs.extend(source.tabs);
            next.panes.retain(|item| item.id != pane);
            next.root = next
                .root
                .clone()
                .without(pane)
                .ok_or("Cannot close the last pane")?;
            if next.active == pane {
                next.active = destination;
            }
            Ok(())
        })
    }
    /// `position` is an insertion slot in the destination before removal.
    pub fn move_tab(
        &mut self,
        source: PaneId,
        tab: TabId,
        destination: PaneId,
        position: usize,
    ) -> Result<(), String> {
        self.change(|next| {
            let from = next
                .panes
                .iter()
                .position(|pane| pane.id == source)
                .ok_or("Source pane no longer exists")?;
            let to = next
                .panes
                .iter()
                .position(|pane| pane.id == destination)
                .ok_or("Destination pane no longer exists")?;
            let index = next.panes[from]
                .tabs
                .iter()
                .position(|item| item.id == tab)
                .ok_or("Tab no longer exists")?;
            if position > next.panes[to].tabs.len()
                || (from != to && next.panes[to].tabs.len() == MAX_TABS)
            {
                return Err("Invalid tab destination".into());
            }
            let moved = next.panes[from].tabs.remove(index);
            let slot = position - usize::from(from == to && index < position);
            next.panes[to].tabs.insert(slot, moved);
            next.panes[to].active_tab = tab;
            if from != to {
                if next.panes[from].tabs.is_empty() {
                    next.panes.remove(from);
                    next.root = next
                        .root
                        .clone()
                        .without(source)
                        .ok_or("Cannot remove the last pane")?;
                } else if next.panes[from].active_tab == tab {
                    next.panes[from].active_tab =
                        next.panes[from].tabs[index.min(next.panes[from].tabs.len() - 1)].id;
                }
            }
            next.active = destination;
            Ok(())
        })
    }
}
impl Node {
    fn validate(
        &self,
        ids: &mut BTreeSet<u64>,
        leaves: &mut BTreeSet<PaneId>,
        depth: usize,
    ) -> Result<(), String> {
        if depth >= MAX_PANES {
            return Err("Layout nesting limit exceeded".into());
        }
        match self {
            Self::Pane { id } => {
                if !ids.insert(id.0) || !leaves.insert(*id) {
                    return Err("Duplicate pane identity".into());
                }
            }
            Self::Split {
                id,
                fraction,
                first,
                second,
                ..
            } => {
                if !ids.insert(id.0) || !(1000..=9000).contains(fraction) {
                    return Err("Invalid divider".into());
                }
                first.validate(ids, leaves, depth + 1)?;
                second.validate(ids, leaves, depth + 1)?;
            }
        }
        Ok(())
    }
    pub fn replace(&mut self, pane: PaneId, node: Self) -> bool {
        match self {
            Self::Pane { id } if *id == pane => {
                *self = node;
                true
            }
            Self::Split { first, second, .. } => {
                first.replace(pane, node.clone()) || second.replace(pane, node)
            }
            _ => false,
        }
    }
    pub fn resize(&mut self, split: SplitId, value: u16) -> bool {
        match self {
            Self::Split { id, fraction, .. } if *id == split => {
                *fraction = value;
                true
            }
            Self::Split { first, second, .. } => {
                first.resize(split, value) || second.resize(split, value)
            }
            _ => false,
        }
    }
    pub fn without(self, pane: PaneId) -> Option<Self> {
        match self {
            Self::Pane { id } => (id != pane).then_some(Self::Pane { id }),
            Self::Split {
                id,
                axis,
                fraction,
                first,
                second,
            } => match (first.without(pane), second.without(pane)) {
                (Some(first), Some(second)) => Some(Self::Split {
                    id,
                    axis,
                    fraction,
                    first: Box::new(first),
                    second: Box::new(second),
                }),
                (node, None) | (None, node) => node,
            },
        }
    }
}

impl Node {
    /// Panes in on-screen order (left-to-right, top-to-bottom).
    pub fn leaves(&self) -> Vec<PaneId> {
        let mut out = vec![];
        self.collect(&mut out);
        out
    }
    fn collect(&self, out: &mut Vec<PaneId>) {
        match self {
            Self::Pane { id } => out.push(*id),
            Self::Split { first, second, .. } => {
                first.collect(out);
                second.collect(out);
            }
        }
    }
    pub fn validate_tree(&self) -> Result<(), String> {
        self.validate(&mut BTreeSet::new(), &mut BTreeSet::new(), 0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}
impl Node {
    fn minimum(&self, width: f32, height: f32, divider: f32) -> (f32, f32) {
        match self {
            Self::Pane { .. } => (width, height),
            Self::Split {
                axis,
                first,
                second,
                ..
            } => {
                let (aw, ah) = first.minimum(width, height, divider);
                let (bw, bh) = second.minimum(width, height, divider);
                match axis {
                    Axis::Horizontal => (aw + bw + divider, ah.max(bh)),
                    Axis::Vertical => (aw.max(bw), ah + bh + divider),
                }
            }
        }
    }
    /// Geometry is computed only on layout/drag changes. It never creates an
    /// animation loop, and never produces overlapping undersized leaves.
    pub fn arrange(
        &self,
        bounds: Rect,
        minimum: (f32, f32),
        divider: f32,
    ) -> Result<Vec<(PaneId, Rect)>, String> {
        self.validate(&mut BTreeSet::new(), &mut BTreeSet::new(), 0)?;
        if ![
            bounds.x,
            bounds.y,
            bounds.width,
            bounds.height,
            minimum.0,
            minimum.1,
            divider,
        ]
        .iter()
        .all(|value| value.is_finite())
            || minimum.0 <= 0.
            || minimum.1 <= 0.
            || divider < 0.
        {
            return Err("Invalid layout geometry".into());
        }
        let required = self.minimum(minimum.0, minimum.1, divider);
        if bounds.width < required.0 || bounds.height < required.1 {
            return Err("Window is too small for this layout".into());
        }
        let mut rows = vec![];
        self.arrange_into(bounds, minimum, divider, &mut rows);
        Ok(rows)
    }
    fn arrange_into(
        &self,
        bounds: Rect,
        minimum: (f32, f32),
        divider: f32,
        rows: &mut Vec<(PaneId, Rect)>,
    ) {
        match self {
            Self::Pane { id } => rows.push((*id, bounds)),
            Self::Split {
                axis,
                fraction,
                first,
                second,
                ..
            } => {
                let a = first.minimum(minimum.0, minimum.1, divider);
                let b = second.minimum(minimum.0, minimum.1, divider);
                let mut left = bounds;
                let mut right = bounds;
                match axis {
                    Axis::Horizontal => {
                        let available = bounds.width - divider;
                        left.width =
                            (available * f32::from(*fraction) / 10000.).clamp(a.0, available - b.0);
                        right.x += left.width + divider;
                        right.width = available - left.width;
                    }
                    Axis::Vertical => {
                        let available = bounds.height - divider;
                        left.height =
                            (available * f32::from(*fraction) / 10000.).clamp(a.1, available - b.1);
                        right.y += left.height + divider;
                        right.height = available - left.height;
                    }
                }
                first.arrange_into(left, minimum, divider, rows);
                second.arrange_into(right, minimum, divider, rows);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestStamp {
    pub pane: PaneId,
    pub tab: TabId,
    pub generation: u64,
}
/// Search controllers will retain this separately from persisted navigation.
#[derive(Default)]
pub struct Requests {
    next: u64,
    live: std::collections::BTreeMap<PaneId, RequestStamp>,
}
impl Requests {
    pub fn begin(&mut self, pane: PaneId, tab: TabId) -> Result<RequestStamp, String> {
        self.next = self
            .next
            .checked_add(1)
            .ok_or("Request generation limit reached")?;
        let stamp = RequestStamp {
            pane,
            tab,
            generation: self.next,
        };
        self.live.insert(pane, stamp);
        Ok(stamp)
    }
    pub fn accepts(&self, stamp: RequestStamp) -> bool {
        self.live.get(&stamp.pane) == Some(&stamp)
    }
    pub fn retire(&mut self, pane: PaneId) {
        self.live.remove(&pane);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two() -> (Session, PaneId) {
        let mut session = Session::initial(View::List);
        let second = session.split(PaneId(1), Axis::Horizontal).unwrap();
        (session, second)
    }

    #[test]
    fn adversarial_sessions_are_rejected() {
        let valid = two().0;
        assert!(valid.validate().is_ok());
        let mut cases = vec![];
        let mut s = valid.clone();
        s.version = 2;
        cases.push(s);
        let mut s = valid.clone();
        s.panes[1].tabs[0].id = s.panes[0].tabs[0].id;
        cases.push(s);
        let mut s = valid.clone();
        s.panes.pop();
        cases.push(s);
        let mut s = valid.clone();
        s.active = PaneId(999);
        cases.push(s);
        let mut s = valid.clone();
        s.next_id = 1;
        cases.push(s);
        let mut s = valid.clone();
        s.panes[0].active_tab = TabId(999);
        cases.push(s);
        let mut s = valid.clone();
        s.panes[0].tabs[0].history_position = 5;
        cases.push(s);
        let mut s = valid.clone();
        s.panes[0].tabs[0].list_offset = [f32::NAN, 0.];
        cases.push(s);
        let mut s = valid.clone();
        s.panes[0].tabs[0].scope = Some("relative".into());
        cases.push(s);
        let mut s = valid.clone();
        if let Node::Split { fraction, .. } = &mut s.root {
            *fraction = 9500;
        }
        cases.push(s);
        let mut s = valid.clone();
        s.root = Node::Split {
            id: SplitId(50),
            axis: Axis::Vertical,
            fraction: 5000,
            first: Box::new(Node::Pane { id: PaneId(1) }),
            second: Box::new(Node::Pane { id: PaneId(1) }),
        };
        cases.push(s);
        for case in cases {
            assert!(case.validate().is_err(), "{case:?}");
        }
    }

    #[test]
    fn failed_changes_leave_the_model_unchanged() {
        let (mut session, second) = two();
        let before = session.clone();
        assert!(session.move_tab(PaneId(1), TabId(999), second, 0).is_err());
        assert!(session.move_tab(PaneId(1), TabId(2), second, 9).is_err());
        assert!(session.resize(SplitId(999), 5000).is_err());
        assert!(session.close(PaneId(1), PaneId(1)).is_err());
        assert_eq!(session, before);
        while session.panes.len() < MAX_PANES {
            session.split(session.active, Axis::Vertical).unwrap();
        }
        let full = session.clone();
        assert!(session.split(session.active, Axis::Vertical).is_err());
        assert_eq!(session, full);
    }

    #[test]
    fn moving_the_last_tab_collapses_its_pane() {
        let (mut session, second) = two();
        let tab = session.pane(second).unwrap().tabs[0].id;
        session.move_tab(second, tab, PaneId(1), 1).unwrap();
        assert_eq!(session.panes.len(), 1);
        assert_eq!(session.root, Node::Pane { id: PaneId(1) });
        assert_eq!(session.pane(PaneId(1)).unwrap().tabs.len(), 2);
        assert_eq!(session.active, PaneId(1));
        session.validate().unwrap();
    }

    #[test]
    fn reordering_within_a_pane_uses_insertion_slots() {
        let mut session = Session::initial(View::List);
        for id in 3..6 {
            let mut tab = Tab::initial(TabId(id), View::List);
            tab.query = id.to_string();
            session.panes[0].tabs.push(tab);
        }
        session.next_id = 6;
        session.move_tab(PaneId(1), TabId(2), PaneId(1), 3).unwrap();
        let order: Vec<_> = session.panes[0].tabs.iter().map(|tab| tab.id.0).collect();
        assert_eq!(order, [3, 4, 2, 5]);
    }

    #[test]
    fn document_panes_are_leaves_but_never_the_focused_pane() {
        let mut session = Session::initial(View::List);
        session.root = Node::Split {
            id: SplitId(3),
            axis: Axis::Horizontal,
            fraction: 5000,
            first: Box::new(Node::Pane { id: PaneId(1) }),
            second: Box::new(Node::Pane { id: PaneId(4) }),
        };
        session.next_id = 5;
        assert!(session.validate().is_err(), "leaf without a pane");
        session.documents.push(DocumentPane {
            id: PaneId(4),
            path: Some(PathBuf::from(r"C:\docs\a.md")),
        });
        session.validate().unwrap();
        let mut focused_document = session.clone();
        focused_document.active = PaneId(4);
        assert!(focused_document.validate().is_err());
        let mut relative = session.clone();
        relative.documents[0].path = Some(PathBuf::from("a.md"));
        assert!(relative.validate().is_err());
        let mut duplicate = session.clone();
        duplicate.documents.push(duplicate.documents[0].clone());
        assert!(duplicate.validate().is_err());
    }

    #[test]
    fn closing_a_pane_keeps_its_tabs() {
        let (mut session, second) = two();
        session.close(second, PaneId(1)).unwrap();
        assert_eq!(session.panes.len(), 1);
        assert_eq!(session.panes[0].tabs.len(), 2);
        assert_eq!(session.active, PaneId(1));
    }

    #[test]
    fn arranged_rectangles_respect_minimums_and_never_overlap() {
        let (mut session, second) = two();
        session.split(second, Axis::Vertical).unwrap();
        let bounds = Rect {
            x: 0.,
            y: 0.,
            width: 900.,
            height: 600.,
        };
        for fraction in [1000, 5000, 9000] {
            let Node::Split { id, .. } = session.root else {
                unreachable!()
            };
            session.resize(id, fraction).unwrap();
            let rows = session.root.arrange(bounds, (240., 160.), 4.).unwrap();
            assert_eq!(rows.len(), 3);
            for (i, (_, a)) in rows.iter().enumerate() {
                assert!(a.width >= 240. && a.height >= 160.);
                assert!(a.x >= 0. && a.y >= 0.);
                assert!(a.x + a.width <= 900.01 && a.y + a.height <= 600.01);
                for (_, b) in &rows[i + 1..] {
                    let apart = a.x + a.width <= b.x
                        || b.x + b.width <= a.x
                        || a.y + a.height <= b.y
                        || b.y + b.height <= a.y;
                    assert!(apart, "{a:?} overlaps {b:?}");
                }
            }
        }
        assert!(
            session
                .root
                .arrange(
                    Rect {
                        width: 300.,
                        ..bounds
                    },
                    (240., 160.),
                    4.
                )
                .is_err()
        );
    }

    #[test]
    fn late_replies_from_another_pane_or_generation_are_rejected() {
        let mut requests = Requests::default();
        let first = requests.begin(PaneId(1), TabId(2)).unwrap();
        let other = requests.begin(PaneId(4), TabId(5)).unwrap();
        assert!(requests.accepts(first) && requests.accepts(other));
        let newer = requests.begin(PaneId(1), TabId(2)).unwrap();
        assert!(!requests.accepts(first));
        assert!(requests.accepts(newer));
        requests.retire(PaneId(4));
        assert!(!requests.accepts(other));
    }
}
