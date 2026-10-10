use crate::{EntityId, EntitySnapshot, SnapshotValue, WorldSnapshot};
use alloc::{
    collections::{BTreeMap, BTreeSet},
    format,
    string::String,
    string::ToString,
    vec::Vec,
};
use bevy_ecs::{
    component::{ComponentId, ComponentInfo},
    entity::Entity,
    hierarchy::ChildOf,
    name::Name,
    reflect::{AppTypeRegistry, ReflectComponent},
    resource::IS_RESOURCE,
    world::{EntityRef, World},
};
use bevy_reflect::{
    serde::{ReflectSerializerProcessor, TypedReflectSerializer},
    PartialReflect, ReflectRef, TypePath, TypeRegistry,
};
use serde::{Serialize, Serializer};
use serde_json::Value;

/// Type-path allow and deny lists for one category of captured values.
///
/// `allow: None` allows every type; `Some(empty_set)` allows none. Deny wins.
/// Paths use [`TypePath::type_path`] for registered types and the ECS type name
/// otherwise. Use explicit paths for dynamically registered components.
#[derive(Clone, Debug, Default)]
pub struct TypeFilter {
    /// An optional allow list of full type paths.
    pub allow: Option<BTreeSet<String>>,
    /// Full type paths to exclude, even if allowed.
    pub deny: BTreeSet<String>,
}

impl TypeFilter {
    /// Restrict capture to these type paths.
    pub fn only(paths: impl IntoIterator<Item = String>) -> Self {
        Self {
            allow: Some(paths.into_iter().collect()),
            deny: BTreeSet::new(),
        }
    }

    /// Add a reflected type to the deny list.
    pub fn deny<T: TypePath>(&mut self) {
        self.deny.insert(T::type_path().to_string());
    }

    fn includes(&self, path: &str) -> bool {
        !self.deny.contains(path) && self.allow.as_ref().is_none_or(|allow| allow.contains(path))
    }
}

/// Controls component and resource inclusion.
///
/// Defaults exclude derived `GlobalTransform`, the reflection registry itself,
/// and all `bevy_time::time::Time<*>` resources. All other types, including opaque
/// ones, are included. Use [`Self::all`] to turn off these exclusions.
#[derive(Clone, Debug)]
pub struct SnapshotConfig {
    /// Component type-path allow and deny lists.
    pub components: TypeFilter,
    /// Resource type-path allow and deny lists.
    pub resources: TypeFilter,
    /// Exclude all generic instantiations of Bevy's `Time` resource.
    /// Set to false to include clocks (subject to the resource filter).
    pub exclude_time_resources: bool,
}

impl SnapshotConfig {
    /// Capture every component and Send + Sync resource, with no noise filters.
    pub fn all() -> Self {
        Self {
            components: TypeFilter::default(),
            resources: TypeFilter::default(),
            exclude_time_resources: false,
        }
    }
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        let mut config = Self::all();
        config
            .components
            .deny
            .insert("bevy_transform::components::global_transform::GlobalTransform".to_string());
        config
            .resources
            .deny
            .insert(core::any::type_name::<AppTypeRegistry>().to_string());
        config.exclude_time_resources = true;
        config
    }
}

impl WorldSnapshot {
    /// Observe a world without mutating it or changing ECS change ticks.
    ///
    /// Uses the world's [`AppTypeRegistry`] if present. Without it, all included
    /// components and resources are still listed, but opaque. Resource entities
    /// are kept separate from normal entities; disabled entities are included.
    pub fn capture(world: &World, config: &SnapshotConfig) -> Self {
        let registry = world.get_resource::<AppTypeRegistry>().map(|r| r.read());
        let registry = registry.as_deref();
        let keys = unique_type_keys(world, registry);
        let mut snapshot = Self::default();
        for entity in world.iter_entities() {
            if entity.contains_id(IS_RESOURCE) {
                continue;
            }
            let mut captured = EntitySnapshot {
                name: entity.get::<Name>().map(|name| name.as_str().to_string()),
                ..Default::default()
            };
            for id in entity.archetype().components() {
                let Some(info) = world.components().get_info(*id) else {
                    continue;
                };
                let path = type_path(info, registry);
                if config.components.includes(&path) {
                    captured
                        .components
                        .insert(keys[id].clone(), capture_value(entity, info, registry));
                }
            }
            snapshot
                .entities
                .insert(EntityId::from(entity.id()), captured);
        }
        for (id, info, _) in world.iter_resources() {
            let path = type_path(info, registry);
            if !config.resources.includes(&path)
                || (config.exclude_time_resources
                    && (path == "bevy_time::time::Time"
                        || path.starts_with("bevy_time::time::Time<")))
            {
                continue;
            }
            if let Some(entity) = world.resource_entities().get(id)
                && let Ok(entity) = world.get_entity(entity)
            {
                snapshot
                    .resources
                    .insert(keys[&id].clone(), capture_value(entity, info, registry));
            }
        }
        snapshot
    }
}

fn type_path(info: &ComponentInfo, registry: Option<&TypeRegistry>) -> String {
    info.type_id().and_then(|id| registry?.get(id)).map_or_else(
        || info.name().to_string(),
        |r| r.type_info().type_path().to_string(),
    )
}

// Type paths normally identify a type, but custom reflection paths and dynamic
// ECS descriptors may collide. Disambiguate against all registered descriptors,
// not just present values, so removing one instance does not rename the others.
fn unique_type_keys(
    world: &World,
    registry: Option<&TypeRegistry>,
) -> BTreeMap<ComponentId, String> {
    let mut groups: BTreeMap<String, Vec<ComponentId>> = BTreeMap::new();
    for (id, info) in world.components().iter_registered() {
        groups
            .entry(type_path(info, registry))
            .or_default()
            .push(id);
    }
    let mut reserved: BTreeSet<String> = groups.keys().cloned().collect();
    let mut keys = BTreeMap::new();
    for (path, mut ids) in groups {
        ids.sort();
        if ids.len() == 1 {
            keys.insert(ids[0], path);
        } else {
            for id in ids {
                let mut key = format!("{path} [component_id:{}]", id.index());
                // A dynamic descriptor could itself use our suffix syntax.
                while !reserved.insert(key.clone()) {
                    key.push('#');
                }
                keys.insert(id, key);
            }
        }
    }
    keys
}

fn opaque(reason: &str) -> SnapshotValue {
    SnapshotValue::Opaque {
        reason: reason.to_string(),
    }
}

fn capture_value(
    entity: EntityRef<'_>,
    info: &ComponentInfo,
    registry: Option<&TypeRegistry>,
) -> SnapshotValue {
    let Some(registry) = registry else {
        return opaque("world has no AppTypeRegistry");
    };
    let Some(registration) = info.type_id().and_then(|id| registry.get(id)) else {
        return opaque("type is not registered for reflection");
    };
    let Some(component) = registration.data::<ReflectComponent>() else {
        return opaque("type has no ReflectComponent metadata");
    };
    let Some(value) = component.reflect(entity) else {
        return opaque("reflected value is unavailable");
    };
    match serde_json::to_value(TypedReflectSerializer::with_processor(
        value.as_partial_reflect(),
        registry,
        &StableSerializer,
    )) {
        Ok(mut value) => {
            canonicalize(&mut value);
            SnapshotValue::Reflected { value }
        }
        // Error text may depend on unordered traversal. A stable reason also
        // ensures failed serialization cannot make repeated captures differ.
        Err(_) => opaque("value cannot be serialized as JSON"),
    }
}

/// Sort JSON maps explicitly rather than relying on `serde_json` feature selection.
pub(crate) fn canonicalize(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for value in map.values_mut() {
                canonicalize(value);
            }
            map.sort_keys();
        }
        Value::Array(values) => {
            for value in values {
                canonicalize(value);
            }
        }
        _ => {}
    }
}

// Reflection normally emits sets in iteration order. Sort their serialized
// elements, but never reorder arrays/lists. Preserve non-finite floats as strings
// rather than JSON null, so NaN and infinities do not become indistinguishable.
struct StableSerializer;

fn contains_entity_reference(value: &Value) -> bool {
    match value {
        Value::Object(object) => {
            object.contains_key("$titan_entity") || object.values().any(contains_entity_reference)
        }
        Value::Array(array) => array.iter().any(contains_entity_reference),
        _ => false,
    }
}

impl ReflectSerializerProcessor for StableSerializer {
    fn try_serialize<S>(
        &self,
        value: &dyn PartialReflect,
        registry: &TypeRegistry,
        serializer: S,
    ) -> Result<Result<S::Ok, S>, S::Error>
    where
        S: Serializer,
    {
        // Intercept typed entities, not arbitrary numbers or strings that happen
        // to equal an ID. Custom serde blobs bypass reflected field traversal.
        if let Some(entity) = value.try_downcast_ref::<Entity>() {
            return serde_json::json!({"$titan_entity": EntityId::from(*entity).to_string()})
                .serialize(serializer)
                .map(Ok);
        }
        // ChildOf's ReflectSerialize serializes the whole tuple as a packed ID,
        // bypassing traversal of its typed Entity field. Handle it explicitly.
        if let Some(parent) = value.try_downcast_ref::<ChildOf>() {
            return self.try_serialize(&parent.parent(), registry, serializer);
        }
        let float = value
            .try_downcast_ref::<f64>()
            .copied()
            .or_else(|| value.try_downcast_ref::<f32>().map(|v| f64::from(*v)));
        if let Some(float) = float
            && !float.is_finite()
        {
            let label = if float.is_nan() {
                "NaN"
            } else if float.is_sign_positive() {
                "+Infinity"
            } else {
                "-Infinity"
            };
            return serializer.serialize_str(label).map(Ok);
        }
        if let ReflectRef::Set(set) = value.reflect_ref() {
            let mut elements = Vec::with_capacity(set.len());
            for element in set.iter() {
                let mut json = serde_json::to_value(TypedReflectSerializer::with_processor(
                    element, registry, self,
                ))
                .map_err(serde::ser::Error::custom)?;
                canonicalize(&mut json);
                elements.push((format!("{json}"), json));
            }
            elements.sort_by(|a, b| a.0.cmp(&b.0));
            let elements: Vec<_> = elements.into_iter().map(|(_, json)| json).collect();
            // Retain set semantics for re-sorting after cross-run normalization.
            if elements.iter().any(contains_entity_reference) {
                return serde_json::json!({"$titan_entity_set": elements})
                    .serialize(serializer)
                    .map(Ok);
            }
            return elements.serialize(serializer).map(Ok);
        }
        if let ReflectRef::Map(map) = value.reflect_ref() {
            let mut entries = Vec::with_capacity(map.len());
            for (key, value) in map.iter() {
                let key = serde_json::to_value(TypedReflectSerializer::with_processor(
                    key, registry, self,
                ))
                .map_err(serde::ser::Error::custom)?;
                entries.push((key, value));
            }
            // JSON object keys cannot be objects. Only maps with typed reference
            // keys need an entry-list representation; ordinary maps stay unchanged.
            if entries
                .iter()
                .any(|(key, _)| contains_entity_reference(key))
            {
                let mut pairs = Vec::with_capacity(entries.len());
                for (key, value) in entries {
                    let value = serde_json::to_value(TypedReflectSerializer::with_processor(
                        value, registry, self,
                    ))
                    .map_err(serde::ser::Error::custom)?;
                    let mut pair = serde_json::json!([key, value]);
                    canonicalize(&mut pair);
                    pairs.push(pair);
                }
                // Full entries break ties when reflection-ignored fields make
                // distinct Rust keys serialize identically. Never drop either entry.
                pairs.sort_by_cached_key(Value::to_string);
                return serde_json::json!({"$titan_entity_map": pairs})
                    .serialize(serializer)
                    .map(Ok);
            }
        }
        Ok(Err(serializer))
    }
}
