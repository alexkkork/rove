//! Keep hash-table growth small and cache bounded repeated hierarchy reads.

use super::Node;
#[path = "world_node_entry.rs"]
mod world_node_entry;
use hashbrown::{hash_table, HashTable};
use std::{
    cell::RefCell, collections::hash_map::RandomState, hash::BuildHasher, ops::Index, rc::Rc,
};
use world_node_entry::NodeEntry;

pub type NodeIter<'a> = std::iter::Map<
    hash_table::Iter<'a, Rc<NodeEntry>>,
    fn(&'a Rc<NodeEntry>) -> (&'a String, &'a Node),
>;
pub type NodeValues<'a> =
    std::iter::Map<hash_table::Iter<'a, Rc<NodeEntry>>, fn(&'a Rc<NodeEntry>) -> &'a Node>;
pub type NodeValuesMut<'a> = std::iter::Map<
    hash_table::IterMut<'a, Rc<NodeEntry>>,
    fn(&'a mut Rc<NodeEntry>) -> &'a mut Node,
>;

#[derive(Default)]
pub struct NodeMap {
    // Node IDs never change while inserted. Hash by that existing ID instead
    // of allocating a second copy in every hash-table entry.
    // Actor snapshots share immutable wrappers; the first mutation detaches
    // only that entry. Each VM still owns its index and hierarchy caches.
    records: HashTable<Rc<NodeEntry>>,
    hashes: RandomState,
    children_cache: RefCell<std::collections::VecDeque<ChildrenCache>>,
    class_cache: RefCell<Option<(String, Option<String>)>>,
    orphan_cache: RefCell<Option<OrphanCache>>,
    #[cfg(test)]
    child_scans: std::cell::Cell<usize>,
}

struct ChildrenCache {
    parent: String,
    ids: Vec<String>,
    dirty: Vec<String>,
}
#[derive(Clone)]
struct OrphanCache {
    ids: std::collections::HashSet<String>,
    dirty: Vec<String>,
}

impl ChildrenCache {
    fn bytes(&self) -> usize {
        self.parent.len()
            + self
                .ids
                .iter()
                .map(|id| id.len() + std::mem::size_of::<String>())
                .sum::<usize>()
    }
}

impl Clone for NodeMap {
    fn clone(&self) -> Self {
        Self {
            records: self.records.clone(),
            hashes: self.hashes.clone(),
            orphan_cache: self.orphan_cache.clone(),
            ..Self::default()
        }
    }
}

impl NodeMap {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            records: HashTable::with_capacity(capacity),
            ..Self::default()
        }
    }
    pub fn len(&self) -> usize {
        self.records.len()
    }
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
    pub fn contains_key(&self, id: &str) -> bool {
        self.records
            .find(self.hashes.hash_one(id), |entry| entry.id() == id)
            .is_some()
    }
    pub fn get(&self, id: &str) -> Option<&Node> {
        self.records
            .find(self.hashes.hash_one(id), |entry| entry.id() == id)
            .map(|entry| entry.node())
    }
    /// Inspect an existing wrapper without expanding a shared received record.
    pub fn peek(&self, id: &str) -> Option<&Node> {
        self.records
            .find(self.hashes.hash_one(id), |entry| entry.id() == id)
            .and_then(|entry| entry.peek_node())
    }
    pub fn get_mut(&mut self, id: &str) -> Option<&mut Node> {
        if let Some(cache) = self.orphan_cache.get_mut() {
            if !cache.dirty.iter().any(|v| v == id) {
                if cache.dirty.len() < 128 && id.len() <= 256 {
                    cache.dirty.push(id.to_owned());
                } else {
                    self.orphan_cache.get_mut().take();
                }
            }
        }
        self.class_cache.get_mut().take();
        // Most mutable accesses change a property, not the hierarchy. Recheck
        // just those IDs on the next read instead of scanning the whole world.
        self.children_cache.get_mut().retain_mut(|cache| {
            if !cache.dirty.iter().any(|dirty| dirty == id) {
                if cache.dirty.len() < 128 && id.len() <= 256 {
                    cache.dirty.push(id.to_owned());
                } else {
                    return false;
                }
            }
            true
        });
        self.records
            .find_mut(self.hashes.hash_one(id), |entry| entry.id() == id)
            .map(|entry| Rc::make_mut(entry).node_mut())
    }
    pub fn insert(&mut self, id: String, node: Node) -> Option<Node> {
        self.orphan_cache.get_mut().take();
        assert_eq!(id, node.id, "node index key must match its immutable ID");
        self.children_cache.get_mut().clear();
        self.class_cache.get_mut().take();
        let hash = self.hashes.hash_one(&id);
        if let Some(slot) = self.records.find_mut(hash, |entry| entry.id() == id) {
            return Some(
                Rc::unwrap_or_clone(std::mem::replace(slot, Rc::new(NodeEntry::new(node))))
                    .into_node(),
            );
        }
        self.records
            .insert_unique(hash, Rc::new(NodeEntry::new(node)), |entry| {
                self.hashes.hash_one(entry.id())
            });
        None
    }
    /// Drop pristine wrappers for received parts further than `keep_radius`
    /// (in XZ) from `origin` back to their shared record. Local overrides,
    /// attributes, revisions and nearby geometry stay fully materialized.
    pub fn prune_far(&mut self, origin: Option<[f64; 3]>, keep_radius: f64, budget: usize) -> usize {
        let Some(origin) = origin else { return 0 };
        let radius2 = keep_radius * keep_radius;
        let candidates: Vec<String> = self
            .records
            .iter()
            .filter(|entry| entry.peek_node().is_none_or(is_prunable_part))
            .filter(|entry| {
                entry.peek_node().and_then(node_position).is_some_and(|p| {
                    let dx = p[0] - origin[0];
                    let dz = p[2] - origin[2];
                    dx * dx + dz * dz > radius2
                })
            })
            .take(budget)
            .map(|entry| entry.id().to_owned())
            .collect();
        let mut pruned = 0;
        for id in candidates {
            let hash = self.hashes.hash_one(id.as_str());
            let Some(entry) = self
                .records
                .find_mut(hash, |candidate| candidate.id() == id.as_str())
            else {
                continue;
            };
            let entry = Rc::make_mut(entry);
            if !entry.is_stub() {
                entry.prune_if_pristine();
                pruned += 1;
            }
        }
        pruned
    }

    pub fn compact_destroyed(&mut self, id: &str) {
        if let Some(entry) = self
            .records
            .find_mut(self.hashes.hash_one(id), |entry| entry.id() == id)
        {
            Rc::make_mut(entry).compact_destroyed();
        }
    }
    pub fn remove(&mut self, id: &str) -> Option<Node> {
        self.orphan_cache.get_mut().take();
        self.children_cache.get_mut().clear();
        self.class_cache.get_mut().take();
        self.records
            .find_entry(self.hashes.hash_one(id), |entry| entry.id() == id)
            .ok()
            .map(|entry| Rc::unwrap_or_clone(entry.remove().0).into_node())
    }
    pub fn iter(&self) -> NodeIter<'_> {
        self.records.iter().map(|entry| {
            let node = entry.node();
            (&node.id, node)
        })
    }
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.records.iter().map(|entry| &entry.node().id)
    }
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.records.iter().map(|entry| entry.id())
    }
    pub fn shared_id(&self, id: &str) -> Option<std::sync::Arc<str>> {
        self.records
            .find(self.hashes.hash_one(id), |entry| entry.id() == id)
            .map(|entry| entry.shared_id())
    }
    pub fn class_name(&self, id: &str) -> Option<&str> {
        self.records
            .find(self.hashes.hash_one(id), |entry| entry.id() == id)
            .map(|entry| entry.class_name())
    }
    pub(crate) fn is_live(&self, id: &str) -> bool {
        self.records
            .find(self.hashes.hash_one(id), |entry| entry.id() == id)
            .is_some_and(|entry| !entry.destroyed())
    }
    /// Distinguish a missing instance from an instance with no parent, without
    /// allocating a mutable wrapper for a received hierarchy notification.
    pub fn parent(&self, id: &str) -> Option<Option<String>> {
        self.records
            .find(self.hashes.hash_one(id), |entry| entry.id() == id)
            .map(|entry| entry.parent())
    }
    /// A hierarchy traversal needs headers, not materialized property maps.
    pub fn live_parent(&self, id: &str) -> Option<String> {
        self.records
            .find(self.hashes.hash_one(id), |entry| entry.id() == id)
            .filter(|entry| !entry.destroyed())
            .and_then(|entry| entry.parent())
    }
    /// Member fallback keeps received order and compares only the headers.
    pub fn child_named<S: AsRef<str>>(
        &self,
        parent: &str,
        name: &str,
        order: &[S],
    ) -> Option<String> {
        self.children(parent, order).into_iter().find(|id| {
            self.records
                .find(self.hashes.hash_one(id), |entry| entry.id() == id)
                .is_some_and(|entry| entry.matches_child_name(parent, name))
        })
    }
    pub fn first_live_of_class(&self, class: &str) -> Option<&Node> {
        if let Some((cached_class, id)) = self.class_cache.borrow().as_ref() {
            if cached_class == class {
                return id.as_deref().and_then(|id| self.get(id));
            }
        }
        let entry = self
            .records
            .iter()
            .find(|entry| entry.class_name() == class && !entry.destroyed());
        if class.len() <= 256 && entry.is_none_or(|entry| entry.id().len() <= 256) {
            self.class_cache.replace(Some((
                class.to_owned(),
                entry.map(|entry| entry.id().to_owned()),
            )));
        }
        entry.map(|entry| entry.node())
    }
    pub fn of_class<'a>(&'a self, class: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.records
            .iter()
            .filter(move |entry| entry.class_name() == class)
            .map(|entry| entry.node())
    }
    pub fn matching_class<'a>(
        &'a self,
        predicate: impl Fn(&str) -> bool + 'a,
    ) -> impl Iterator<Item = &'a Node> + 'a {
        self.records
            .iter()
            .filter(move |entry| predicate(entry.class_name()))
            .map(|entry| entry.node())
    }
    pub fn materialized_count(&self) -> usize {
        self.records
            .iter()
            .filter(|entry| entry.peek_node().is_some())
            .count()
    }
    pub fn materialized_values(&self) -> impl Iterator<Item = &Node> {
        self.records.iter().filter_map(|entry| entry.peek_node())
    }
    pub fn child_ids(&self, parent: &str) -> Vec<String> {
        self.records
            .iter()
            .filter(|entry| entry.is_child(parent))
            .map(|entry| entry.id().to_owned())
            .collect()
    }
    pub fn values(&self) -> NodeValues<'_> {
        self.records.iter().map(|entry| entry.node())
    }
    pub fn values_mut(&mut self) -> NodeValuesMut<'_> {
        self.orphan_cache.get_mut().take();
        self.children_cache.get_mut().clear();
        self.class_cache.get_mut().take();
        self.records
            .iter_mut()
            .map(|entry| Rc::make_mut(entry).node_mut())
    }

    /// Missing-parent membership changes only with headers/index membership.
    /// Property-only writes recheck their bounded dirty set, not every node.
    pub fn orphan_ids(&self) -> Vec<String> {
        let orphan = |id: &str| {
            self.parent(id)
                .flatten()
                .is_some_and(|parent| !self.contains_key(&parent))
        };
        let mut slot = self.orphan_cache.borrow_mut();
        if let Some(cache) = slot.as_mut() {
            for id in std::mem::take(&mut cache.dirty) {
                if orphan(&id) {
                    cache.ids.insert(id);
                } else {
                    cache.ids.remove(&id);
                }
            }
        } else {
            *slot = Some(OrphanCache {
                ids: self
                    .ids()
                    .filter(|id| orphan(id))
                    .map(str::to_owned)
                    .collect(),
                dirty: Vec::new(),
            });
        }
        let cache = slot.as_ref().unwrap();
        let ids = cache.ids.iter().cloned().collect::<Vec<_>>();
        if cache.ids.len() > 1024 || cache.ids.iter().map(|s| s.len() + 32).sum::<usize>() > 65_536
        {
            slot.take();
        }
        ids
    }

    /// Validate mutable accesses, including rollback and Actor writes, before
    /// reusing the cache. Returned vectors are independent of cached results.
    pub fn children<S: AsRef<str>>(&self, parent: &str, order: &[S]) -> Vec<String> {
        {
            let mut caches = self.children_cache.borrow_mut();
            if let Some(index) = caches.iter().position(|cache| cache.parent == parent) {
                let mut cache = caches.remove(index).unwrap();
                if cache.dirty.iter().all(|id| {
                    let was_child = cache.ids.iter().any(|child| child == id);
                    let is_child = self
                        .records
                        .find(self.hashes.hash_one(id), |entry| entry.id() == id)
                        .is_some_and(|entry| entry.is_child(parent));
                    was_child == is_child
                }) {
                    cache.dirty.clear();
                    let result = cache.ids.clone();
                    caches.push_back(cache);
                    return result;
                }
            }
        }
        #[cfg(test)]
        self.child_scans.set(self.child_scans.get() + 1);
        let result = order
            .iter()
            .filter_map(|id| {
                let id = id.as_ref();
                self.records
                    .find(self.hashes.hash_one(id), |entry| entry.id() == id)
                    .filter(|entry| entry.is_child(parent))
                    .map(|entry| entry.id().to_owned())
            })
            .collect::<Vec<_>>();
        // Keep short repeated paths warm without retaining an all-world index.
        // IDs use at most 64 KiB across eight parents. Each entry separately
        // bounds its dirty-ID validation journal to 128 IDs of at most 256 bytes.
        let bytes = parent.len()
            + result
                .iter()
                .map(|id| id.len() + std::mem::size_of::<String>())
                .sum::<usize>();
        if result.len() <= 1024 && bytes <= 65_536 {
            let mut caches = self.children_cache.borrow_mut();
            while caches.len() >= 8
                || bytes + caches.iter().map(ChildrenCache::bytes).sum::<usize>() > 65_536
            {
                if caches.pop_front().is_none() {
                    break;
                }
            }
            caches.push_back(ChildrenCache {
                parent: parent.to_owned(),
                ids: result.clone(),
                dirty: Vec::new(),
            });
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, parent: &str) -> Node {
        Node {
            id: id.into(),
            class_name: "Folder".into(),
            name: id.into(),
            parent: Some(parent.into()),
            destroyed: false,
            server_owned: false,
            received_creation_complete: false,
            attributes_known: true,
            collection_tags_known: true,
            projected: None,
            data: None,
        }
    }

    #[test]
    fn actor_forks_share_untouched_entries_but_detach_all_mutation_paths() {
        use serde_json::json;
        let mut root = NodeMap::default();
        for id in ["a", "b", "c"] {
            let mut n = node(id, "root");
            n.attributes.insert("score".into(), json!(1));
            root.insert(id.into(), n);
        }
        let mut actor = root.clone();
        assert!(std::ptr::eq(
            root.get("a").unwrap(),
            actor.get("a").unwrap()
        ));
        actor
            .get_mut("a")
            .unwrap()
            .attributes
            .insert("score".into(), json!(2));
        assert_eq!(root.get("a").unwrap().attributes["score"], 1);
        assert_eq!(actor.get("a").unwrap().attributes["score"], 2);
        assert!(std::ptr::eq(
            root.get("b").unwrap(),
            actor.get("b").unwrap()
        ));
        let mut removed = actor.remove("b").unwrap();
        removed.attributes.insert("score".into(), json!(3));
        assert_eq!(root.get("b").unwrap().attributes["score"], 1);
        let mut previous = actor.insert("c".into(), node("c", "changed")).unwrap();
        previous.name = "detached".into();
        assert_eq!(root.get("c").unwrap().name, "c");
        assert_eq!(actor.get("c").unwrap().parent.as_deref(), Some("changed"));
        let mut second = root.clone();
        for n in second.values_mut() {
            n.destroyed = true;
        }
        assert!(root.values().all(|n| !n.destroyed));
        assert!(second.values().all(|n| n.destroyed));
        root.get_mut("b").unwrap().name = "root edited".into();
        assert_eq!(second.get("b").unwrap().name, "b");
    }

    #[test]
    fn cached_orphans_follow_parent_arrival_removal_reparent_and_actor_edits() {
        let mut root = NodeMap::default();
        root.insert("a".into(), node("a", "parent"));
        assert_eq!(root.orphan_ids(), vec!["a"]);
        root.insert("parent".into(), node("parent", "parent"));
        assert!(root.orphan_ids().is_empty());
        let mut actor = root.clone();
        actor.get_mut("a").unwrap().parent = Some("missing".into());
        assert_eq!(actor.orphan_ids(), vec!["a"]);
        assert!(root.orphan_ids().is_empty());
        root.remove("parent");
        assert_eq!(root.orphan_ids(), vec!["a"]);
        for n in root.values_mut() {
            n.parent = None;
        }
        assert!(root.orphan_ids().is_empty());
        for i in 0..140 {
            root.get_mut(&format!("absent-{i}"));
        }
        assert!(root.orphan_ids().is_empty());
    }

    #[test]
    fn named_paths_share_a_bounded_cache_and_follow_renames_and_reparents() {
        let mut nodes = NodeMap::default();
        let order = vec!["a", "b", "c", "later"];
        for (id, parent) in [("a", "root"), ("b", "a"), ("c", "b"), ("later", "b")] {
            nodes.insert(id.into(), node(id, parent));
        }
        for _ in 0..100 {
            assert_eq!(nodes.child_named("root", "a", &order).as_deref(), Some("a"));
            assert_eq!(nodes.child_named("a", "b", &order).as_deref(), Some("b"));
            assert_eq!(nodes.child_named("b", "c", &order).as_deref(), Some("c"));
            nodes.get_mut("c").unwrap().name = "c".into();
        }
        assert_eq!(nodes.child_scans.get(), 3);
        nodes.get_mut("later").unwrap().name = "c".into();
        assert_eq!(nodes.child_named("b", "c", &order).as_deref(), Some("c"));
        nodes.get_mut("c").unwrap().name = "renamed".into();
        assert_eq!(
            nodes.child_named("b", "c", &order).as_deref(),
            Some("later")
        );
        nodes.get_mut("c").unwrap().parent = Some("root".into());
        assert!(nodes.child_named("b", "renamed", &order).is_none());
        assert_eq!(
            nodes.child_named("root", "renamed", &order).as_deref(),
            Some("c")
        );
        nodes.get_mut("c").unwrap().destroyed = true;
        assert!(nodes.child_named("root", "renamed", &order).is_none());
        for i in 0..100 {
            nodes.children(&format!("missing-{i}"), &order);
        }
        let caches = nodes.children_cache.borrow();
        assert!(caches.len() <= 8);
        assert!(caches.iter().map(ChildrenCache::bytes).sum::<usize>() <= 65_536);
    }

    #[test]
    fn cached_class_query_tracks_live_membership_and_missing_classes() {
        let mut nodes = NodeMap::default();
        assert!(nodes.first_live_of_class("Folder").is_none());
        nodes.insert("a".into(), node("a", "root"));
        assert_eq!(nodes.first_live_of_class("Folder").unwrap().id, "a");
        nodes.get_mut("a").unwrap().destroyed = true;
        assert!(nodes.first_live_of_class("Folder").is_none());
        nodes.get_mut("a").unwrap().destroyed = false;
        assert_eq!(nodes.first_live_of_class("Folder").unwrap().id, "a");
        nodes.get_mut("a").unwrap().class_name = "Part".into();
        assert!(nodes.first_live_of_class("Folder").is_none());
        assert_eq!(nodes.first_live_of_class("Part").unwrap().id, "a");
        nodes.remove("a");
        assert!(nodes.first_live_of_class("Part").is_none());
    }

    #[test]
    fn property_mutations_keep_cache_but_topology_changes_rebuild_it() {
        let mut nodes = NodeMap::default();
        let mut order = vec!["a".to_owned(), "b".to_owned()];
        nodes.insert("a".into(), node("a", "root"));
        nodes.insert("b".into(), node("b", "elsewhere"));
        assert_eq!(nodes.children("root", &order), vec!["a"]);
        for i in 0..200 {
            nodes.get_mut("a").unwrap().name = format!("name-{i}");
            nodes.get_mut("b").unwrap().name = format!("other-{i}");
            assert_eq!(nodes.children("root", &order), vec!["a"]);
        }
        assert_eq!(
            nodes.child_scans.get(),
            1,
            "property changes do not rescan nodes"
        );
        nodes.get_mut("b").unwrap().parent = Some("root".into());
        assert_eq!(nodes.children("root", &order), vec!["a", "b"]);
        nodes.get_mut("a").unwrap().destroyed = true;
        assert_eq!(nodes.children("root", &order), vec!["b"]);
        nodes.remove("b");
        assert!(nodes.children("root", &order).is_empty());
        nodes.insert("c".into(), node("c", "root"));
        order.push("c".into());
        assert_eq!(nodes.children("root", &order), vec!["c"]);
        assert_eq!(nodes.child_scans.get(), 5);
    }

    #[test]
    fn dirty_cache_is_bounded_and_reparent_then_restore_is_safe() {
        let mut nodes = NodeMap::default();
        let order = (0..130).map(|i| i.to_string()).collect::<Vec<_>>();
        for id in &order {
            nodes.insert(id.clone(), node(id, "root"));
        }
        assert_eq!(nodes.children("root", &order), order);
        nodes.get_mut("0").unwrap().parent = None;
        nodes.get_mut("0").unwrap().parent = Some("root".into());
        assert_eq!(nodes.children("root", &order), order);
        assert_eq!(nodes.child_scans.get(), 1);
        for id in &order {
            nodes.get_mut(id).unwrap().destroyed = true;
        }
        assert!(nodes.children_cache.borrow().is_empty());
        assert!(nodes.children("root", &order).is_empty());
        assert_eq!(nodes.child_scans.get(), 2);
    }
}

impl<Q: ?Sized + AsRef<str>> Index<&Q> for NodeMap {
    type Output = Node;
    fn index(&self, key: &Q) -> &Node {
        self.get(key.as_ref()).expect("no entry found for key")
    }
}

/// Only received plain parts with a complete shared record are prunable. The
/// projected record has to answer `Size`/`CFrame` because the wrapper that
/// carries them goes away.
fn is_prunable_part(node: &super::Node) -> bool {
    if !node.server_owned || node.class_name != "Part" {
        return false;
    }
    let Some(projected) = node.projected.as_ref() else {
        return false;
    };
    projected.property("Size").ok().flatten().is_some()
        && projected.property("CFrame").ok().flatten().is_some()
}

/// World-space position of a part, from its received/override CFrame.
fn node_position(node: &super::Node) -> Option<[f64; 3]> {
    let cframe = node
        .local_overrides
        .get("CFrame")
        .or_else(|| node.properties.get("CFrame"))?;
    let p = cframe.get("$cframe")?.get("position")?.as_array()?;
    Some([
        p.first()?.as_f64()?,
        p.get(1)?.as_f64()?,
        p.get(2)?.as_f64()?,
    ])
}
