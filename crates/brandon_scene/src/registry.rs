use crate::Behaviour;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

type DeserializeFn =
    fn(&mut dyn erased_serde::Deserializer) -> Result<Box<dyn Behaviour>, erased_serde::Error>;

pub(crate) struct Entry {
    pub(crate) name: String,
    pub(crate) deserialize: DeserializeFn,
    pub(crate) serialize: fn(&dyn Behaviour) -> &dyn erased_serde::Serialize,
    pub(crate) clone: fn(&dyn Behaviour) -> Box<dyn Behaviour>,
}

#[derive(Default)]
struct Registry {
    by_name: HashMap<String, Arc<Entry>>,
    by_type: HashMap<TypeId, Arc<Entry>>,
}

fn registry() -> &'static RwLock<Registry> {
    static REGISTRY: OnceLock<RwLock<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(Default::default)
}

fn deserialize_as<T: Behaviour + DeserializeOwned>(
    d: &mut dyn erased_serde::Deserializer,
) -> Result<Box<dyn Behaviour>, erased_serde::Error> {
    Ok(Box::new(erased_serde::deserialize::<T>(d)?))
}

fn downcast<T: Behaviour>(b: &dyn Behaviour) -> &T {
    (b as &dyn Any)
        .downcast_ref::<T>()
        .expect("behaviour registry entry used with the wrong type")
}

fn serialize_as<T: Behaviour + Serialize>(b: &dyn Behaviour) -> &dyn erased_serde::Serialize {
    downcast::<T>(b)
}

fn clone_as<T: Behaviour + Clone>(b: &dyn Behaviour) -> Box<dyn Behaviour> {
    Box::new(downcast::<T>(b).clone())
}

/// Makes a behaviour type usable in scene files, under its type name.
///
/// Call this once at startup for every behaviour that appears in a scene
/// file. Registering the same type twice is fine.
pub fn register<T>()
where
    T: Behaviour + Serialize + DeserializeOwned + Clone,
{
    let full = std::any::type_name::<T>();
    let short = full
        .split('<')
        .next()
        .unwrap_or(full)
        .rsplit("::")
        .next()
        .unwrap_or(full);
    register_as::<T>(short);
}

/// Like [`register`], but with an explicit name to use in scene files.
pub fn register_as<T>(name: &str)
where
    T: Behaviour + Serialize + DeserializeOwned + Clone,
{
    let entry = Arc::new(Entry {
        name: name.to_string(),
        deserialize: deserialize_as::<T>,
        serialize: serialize_as::<T>,
        clone: clone_as::<T>,
    });
    let mut reg = registry().write().unwrap();
    if let Some(existing) = reg.by_name.get(name) {
        if !reg
            .by_type
            .get(&TypeId::of::<T>())
            .is_some_and(|e| Arc::ptr_eq(e, existing))
        {
            log::warn!("behaviour name `{name}` was already registered for another type and is now replaced");
        }
    }
    reg.by_name.insert(name.to_string(), entry.clone());
    reg.by_type.insert(TypeId::of::<T>(), entry);
}

pub(crate) fn entry_by_name(name: &str) -> Option<Arc<Entry>> {
    registry().read().unwrap().by_name.get(name).cloned()
}

pub(crate) fn entry_for(behaviour: &dyn Behaviour) -> Option<Arc<Entry>> {
    let type_id = (behaviour as &dyn Any).type_id();
    registry().read().unwrap().by_type.get(&type_id).cloned()
}

pub(crate) fn known_names() -> Vec<String> {
    let mut names: Vec<String> = registry().read().unwrap().by_name.keys().cloned().collect();
    names.sort();
    names
}

static KEEP_UNKNOWN: AtomicBool = AtomicBool::new(false);

/// Chooses what happens when a scene file names a behaviour that is not
/// registered.
///
/// Normally that is a mistake and loading fails. Tools that open scenes
/// without the game's code, such as the editor, turn this on: unknown
/// behaviours are then kept as [`RawBehaviour`](crate::RawBehaviour)s, which
/// do nothing but are written back unchanged when the scene is saved.
pub fn keep_unknown_behaviours(keep: bool) {
    KEEP_UNKNOWN.store(keep, Ordering::Relaxed);
}

pub(crate) fn keeps_unknown() -> bool {
    KEEP_UNKNOWN.load(Ordering::Relaxed)
}
