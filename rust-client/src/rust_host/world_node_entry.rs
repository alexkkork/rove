//! Keep untouched received instances as a shared record handle. The full host
//! wrapper is created only when a caller actually needs its mutable fields.

use super::Node;
use crate::engine_world_projection::ProjectedNode;
use rove_client_core::world::ReadOnlyField;
use std::cell::OnceCell;

#[derive(Clone)]
pub struct NodeEntry {
    node: OnceCell<Box<Node>>,
    projected: Option<ProjectedNode>,
    flags: u8,
}

impl NodeEntry {
    pub fn new(node: Node) -> Self {
        if let Some(projected) = node.projected.as_ref().filter(|_| node.data.is_none()) {
            // Snapshot sidecars, local headers and clones can differ from the
            // received record. Keep a full wrapper in those cases.
            let name = projected
                .name()
                .unwrap_or_else(|_| projected.class_name().to_owned());
            let parent = match projected.parent() {
                ReadOnlyField::Value(value) => Some(value),
                ReadOnlyField::Missing | ReadOnlyField::Null => None,
            };
            if node.id == projected.id()
                && node.class_name == projected.class_name()
                && node.name == name
                && node.parent == parent
            {
                let flags = u8::from(node.destroyed)
                    | (u8::from(node.server_owned) << 1)
                    | (u8::from(node.attributes_known) << 2)
                    | (u8::from(node.collection_tags_known) << 3);
                return Self {
                    node: OnceCell::new(),
                    projected: Some(projected.clone()),
                    flags,
                };
            }
        }
        Self {
            node: OnceCell::from(Box::new(node)),
            projected: None,
            flags: 0,
        }
    }
    pub fn id(&self) -> &str {
        self.node.get().map_or_else(
            || self.projected.as_ref().unwrap().id(),
            |node| node.id.as_str(),
        )
    }
    pub fn shared_id(&self) -> std::sync::Arc<str> {
        if let Some(node) = self.node.get() {
            if let Some(projected) = node.projected.as_ref().filter(|p| p.id() == node.id) {
                return projected.raw().shared_id();
            }
            return std::sync::Arc::from(node.id.as_str());
        }
        self.projected.as_ref().unwrap().raw().shared_id()
    }
    pub fn class_name(&self) -> &str {
        self.node.get().map_or_else(
            || self.projected.as_ref().unwrap().class_name(),
            |node| node.class_name.as_str(),
        )
    }
    pub fn destroyed(&self) -> bool {
        self.node
            .get()
            .map_or(self.flags & 1 != 0, |node| node.destroyed)
    }
    pub fn parent(&self) -> Option<String> {
        self.node.get().map_or_else(
            || {
                if self.flags & 16 != 0 {
                    None
                } else {
                    match self.projected.as_ref().unwrap().parent() {
                        ReadOnlyField::Value(value) => Some(value),
                        ReadOnlyField::Missing | ReadOnlyField::Null => None,
                    }
                }
            },
            |node| node.parent.clone(),
        )
    }
    pub fn matches_child_name(&self, parent: &str, name: &str) -> bool {
        if let Some(node) = self.node.get() {
            return node.parent.as_deref() == Some(parent) && node.name == name;
        }
        if self.flags & 16 != 0 {
            return false;
        }
        let projected = self.projected.as_ref().unwrap();
        projected.parent_matches(parent)
            && projected
                .name()
                .unwrap_or_else(|_| projected.class_name().to_owned())
                == name
    }
    pub fn is_child(&self, parent: &str) -> bool {
        if self.destroyed() {
            return false;
        }
        self.node.get().map_or_else(
            || self.projected.as_ref().unwrap().parent_matches(parent),
            |node| node.parent.as_deref() == Some(parent),
        )
    }
    pub fn node(&self) -> &Node {
        self.node.get_or_init(|| {
            let projected = self.projected.as_ref().unwrap();
            Box::new(Node {
                id: projected.id().to_owned(),
                class_name: projected.class_name().to_owned(),
                name: projected
                    .name()
                    .unwrap_or_else(|_| projected.class_name().to_owned()),
                parent: if self.flags & 16 != 0 {
                    None
                } else {
                    match projected.parent() {
                        ReadOnlyField::Value(v) => Some(v),
                        _ => None,
                    }
                },
                destroyed: self.flags & 1 != 0,
                server_owned: self.flags & 2 != 0,
                received_creation_complete: projected.raw().record_field("received_creation_complete") == Some(serde_json::Value::Bool(true)),
                attributes_known: self.flags & 4 != 0,
                collection_tags_known: self.flags & 8 != 0,
                projected: Some(projected.clone()),
                data: if self.flags & 16 != 0 {
                    let mut data = super::super::NodeData::default();
                    data.local_overrides
                        .insert("Parent".into(), serde_json::Value::Null);
                    Some(Box::new(data))
                } else {
                    None
                },
            })
        })
    }
    pub fn node_mut(&mut self) -> &mut Node {
        self.node();
        // A later received update can replace Node.projected. Release this
        // duplicate handle first so the entry cannot pin an obsolete record.
        self.projected = None;
        self.node.get_mut().unwrap()
    }
    pub fn into_node(mut self) -> Node {
        self.node_mut();
        *self.node.into_inner().unwrap()
    }
    /// Destroy does not need a full wrapper solely for a nil Parent override.
    /// Keep the immutable record so surviving Luau handles can still read it.
    pub fn compact_destroyed(&mut self) {
        let Some(node) = self.node.get() else {
            return;
        };
        let Some(projected) = node.projected.as_ref() else {
            return;
        };
        if !node.destroyed
            || node.parent.is_some()
            || !node.properties.is_empty()
            || !node.attributes.is_empty()
            || !node.revisions.is_empty()
            || !node.collection_tags.is_empty()
            || node.local_revision != 0
            || node.local_overrides.len() != 1
            || node.local_overrides.get("Parent") != Some(&serde_json::Value::Null)
            || node.id != projected.id()
            || node.class_name != projected.class_name()
            || node.name
                != projected
                    .name()
                    .unwrap_or_else(|_| projected.class_name().to_owned())
        {
            return;
        }
        self.flags = 1
            | 16
            | (u8::from(node.server_owned) << 1)
            | (u8::from(node.attributes_known) << 2)
            | (u8::from(node.collection_tags_known) << 3);
        self.projected = Some(projected.clone());
        self.node.take();
    }
    pub fn peek_node(&self) -> Option<&Node> {
        self.node.get().map(Box::as_ref)
    }
    /// True when the wrapper is already just its shared record.
    pub fn is_stub(&self) -> bool {
        self.node.get().is_none() && self.projected.is_some()
    }
    /// Drop a pristine far node back to its shared record. Anything the script
    /// layer may have touched (local overrides, attributes, revisions, tags,
    /// lifecycle flags) keeps the full wrapper.
    pub fn prune_if_pristine(&mut self) {
        let Some(node) = self.node.get() else {
            return;
        };
        // Plain snapshots keep properties inline (no projected record), so the
        // pristine check below still applies; only the stored copy differs.
        let projected = node.projected.clone();
        // `node.take()` discards the wrapper's own property map, so each one
        // must be identical in the shared record.
        let properties_safe = node.properties.iter().all(|(key, value)| {
            projected.as_ref().is_some_and(|p| {
                p.property(key)
                    .ok()
                    .flatten()
                    .is_some_and(|shared| shared == *value)
            })
        });
        if !node.attributes_known
            || !properties_safe
            || node.destroyed
            || !node.local_overrides.is_empty()
            || !node.attributes.is_empty()
            || !node.revisions.is_empty()
            || !node.collection_tags.is_empty()
            || node.local_revision != 0
            || projected.as_ref().is_some_and(|projected| {
                node.id != projected.id()
                    || node.class_name != projected.class_name()
                    || node.name
                        != projected
                            .name()
                            .unwrap_or_else(|_| projected.class_name().to_owned())
                    || node.parent
                        != match projected.parent() {
                            ReadOnlyField::Value(value) => Some(value.to_owned()),
                            ReadOnlyField::Missing | ReadOnlyField::Null => None,
                        }
            })
        {
            return;
        }
        self.flags = (u8::from(node.server_owned) << 1)
            | (u8::from(node.attributes_known) << 2)
            | (u8::from(node.collection_tags_known) << 3);
        if let Some(projected) = projected {
            self.projected = Some(projected);
            self.node.take();
        }
    }
}
