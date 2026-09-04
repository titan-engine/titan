use super::{
    Commands, Query, QueryData,
    access::{AccessMode, AccessTarget, SystemAccess, SystemContext},
};
use std::{
    any::TypeId,
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

mod sealed {
    pub trait Sealed {}
}
/// Sealed parameters supported by typed systems.
pub trait SystemParam: sealed::Sealed {
    type Item<'w>;
    #[doc(hidden)]
    fn accesses() -> Vec<SystemAccess>;
    #[doc(hidden)]
    fn fetch<'w>(context: &mut SystemContext<'w>) -> Self::Item<'w>;
}
/// Shared access to a required resource.
pub struct Res<'w, T: Send + Sync + 'static>(&'w T);
impl<T: Send + Sync + 'static> Deref for Res<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.0
    }
}
/// Exclusive access to a required resource.
pub struct ResMut<'w, T: Send + Sync + 'static>(&'w mut T);
impl<T: Send + Sync + 'static> Deref for ResMut<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.0
    }
}
impl<T: Send + Sync + 'static> DerefMut for ResMut<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.0
    }
}
impl<T: Send + Sync + 'static> sealed::Sealed for Res<'_, T> {}
impl<T: Send + Sync + 'static> SystemParam for Res<'_, T> {
    type Item<'w> = Res<'w, T>;
    fn accesses() -> Vec<SystemAccess> {
        vec![SystemAccess::typed::<T>(
            AccessTarget::Resource,
            AccessMode::Read,
        )]
    }
    fn fetch<'w>(context: &mut SystemContext<'w>) -> Self::Item<'w> {
        Res(context.shared_resources[&TypeId::of::<T>()]
            .downcast_ref()
            .expect("validated resource type"))
    }
}
impl<T: Send + Sync + 'static> sealed::Sealed for ResMut<'_, T> {}
impl<T: Send + Sync + 'static> SystemParam for ResMut<'_, T> {
    type Item<'w> = ResMut<'w, T>;
    fn accesses() -> Vec<SystemAccess> {
        vec![SystemAccess::typed::<T>(
            AccessTarget::Resource,
            AccessMode::Write,
        )]
    }
    fn fetch<'w>(context: &mut SystemContext<'w>) -> Self::Item<'w> {
        ResMut(
            context
                .mutable_resources
                .remove(&TypeId::of::<T>())
                .expect("validated resource access")
                .downcast_mut()
                .expect("validated resource type"),
        )
    }
}
impl<Q: QueryData> sealed::Sealed for Query<'_, Q> {}
impl<Q: QueryData> SystemParam for Query<'_, Q> {
    type Item<'w> = Query<'w, Q>;
    fn accesses() -> Vec<SystemAccess> {
        Q::accesses()
    }
    fn fetch<'w>(context: &mut SystemContext<'w>) -> Self::Item<'w> {
        Query {
            state: Q::fetch(context),
            marker: PhantomData,
        }
    }
}
impl sealed::Sealed for Commands<'_> {}
impl SystemParam for Commands<'_> {
    type Item<'w> = Commands<'w>;
    fn accesses() -> Vec<SystemAccess> {
        vec![SystemAccess::typed::<Commands<'static>>(
            AccessTarget::Commands,
            AccessMode::Write,
        )]
    }
    fn fetch<'w>(context: &mut SystemContext<'w>) -> Self::Item<'w> {
        context.commands.take().expect("validated command access")
    }
}
