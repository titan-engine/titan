use super::{
    Component, Entity,
    access::{AccessMode, AccessTarget, SystemAccess, SystemContext},
    storage::ComponentStorage,
};
use std::{any::TypeId, marker::PhantomData};

mod sealed {
    pub trait Sealed {}
    impl<T: super::Component> Sealed for &T {}
    impl<T: super::Component> Sealed for &mut T {}
}
/// A component reference or tuple of component references, up to arity four.
/// Implementations are sealed so access declarations cannot bypass validation.
pub trait QueryData: sealed::Sealed {
    #[doc(hidden)]
    type State<'w>;
    type Item<'a>;
    #[doc(hidden)]
    fn accesses() -> Vec<SystemAccess>;
    #[doc(hidden)]
    fn fetch<'w>(context: &mut SystemContext<'w>) -> Self::State<'w>;
    #[doc(hidden)]
    fn entities(state: &Self::State<'_>) -> Vec<Entity>;
    #[doc(hidden)]
    fn get<'a, 'w: 'a>(state: &'a mut Self::State<'w>, entity: Entity) -> Option<Self::Item<'a>>;
}
#[doc(hidden)]
pub struct ReadStorage<'w, T>(Option<&'w ComponentStorage<T>>);
#[doc(hidden)]
pub struct WriteStorage<'w, T>(Option<&'w mut ComponentStorage<T>>);
impl<T: Component> QueryData for &T {
    type State<'w> = ReadStorage<'w, T>;
    type Item<'a> = &'a T;
    fn accesses() -> Vec<SystemAccess> {
        vec![SystemAccess::typed::<T>(
            AccessTarget::Component,
            AccessMode::Read,
        )]
    }
    fn fetch<'w>(context: &mut SystemContext<'w>) -> Self::State<'w> {
        ReadStorage(
            context
                .shared_components
                .get(&TypeId::of::<T>())
                .and_then(|storage| storage.as_any().downcast_ref()),
        )
    }
    fn entities(state: &Self::State<'_>) -> Vec<Entity> {
        state
            .0
            .into_iter()
            .flat_map(|storage| storage.iter().map(|(entity, _)| entity))
            .collect()
    }
    fn get<'a, 'w: 'a>(state: &'a mut Self::State<'w>, entity: Entity) -> Option<Self::Item<'a>> {
        state.0?.get(entity)
    }
}
impl<T: Component> QueryData for &mut T {
    type State<'w> = WriteStorage<'w, T>;
    type Item<'a> = &'a mut T;
    fn accesses() -> Vec<SystemAccess> {
        vec![SystemAccess::typed::<T>(
            AccessTarget::Component,
            AccessMode::Write,
        )]
    }
    fn fetch<'w>(context: &mut SystemContext<'w>) -> Self::State<'w> {
        WriteStorage(
            context
                .mutable_components
                .remove(&TypeId::of::<T>())
                .and_then(|storage| storage.as_any_mut().downcast_mut()),
        )
    }
    fn entities(state: &Self::State<'_>) -> Vec<Entity> {
        state
            .0
            .as_ref()
            .into_iter()
            .flat_map(|storage| storage.iter().map(|(entity, _)| entity))
            .collect()
    }
    fn get<'a, 'w: 'a>(state: &'a mut Self::State<'w>, entity: Entity) -> Option<Self::Item<'a>> {
        state.0.as_mut()?.get_mut(entity)
    }
}
macro_rules! tuple_query {
    ($first:ident:$first_index:tt $(,$name:ident:$index:tt)*) => {
        impl<$first: QueryData, $($name: QueryData),*> sealed::Sealed for ($first, $($name,)*) {}
        impl<$first: QueryData, $($name: QueryData),*> QueryData for ($first, $($name,)*) {
            type State<'w> = ($first::State<'w>, $($name::State<'w>,)*);
            type Item<'a> = ($first::Item<'a>, $($name::Item<'a>,)*);
            fn accesses() -> Vec<SystemAccess> { let mut accesses = Vec::new(); accesses.extend($first::accesses()); $(accesses.extend($name::accesses());)* accesses }
            fn fetch<'w>(context: &mut SystemContext<'w>) -> Self::State<'w> { ($first::fetch(context), $($name::fetch(context),)*) }
            fn entities(state: &Self::State<'_>) -> Vec<Entity> { $first::entities(&state.$first_index) }
            fn get<'a, 'w: 'a>(state: &'a mut Self::State<'w>, entity: Entity) -> Option<Self::Item<'a>> { Some(($first::get(&mut state.$first_index, entity)?, $($name::get(&mut state.$index, entity)?,)*)) }
        }
    };
}
tuple_query!(A:0);
tuple_query!(A:0,B:1);
tuple_query!(A:0,B:1,C:2);
tuple_query!(A:0,B:1,C:2,D:3);

/// A validated view of component storage borrowed for one system invocation.
/// Rows use the first component's dense order; removals can change that order.
/// The callback's references cannot escape the row visit.
pub struct Query<'w, Q: QueryData> {
    pub(crate) state: Q::State<'w>,
    pub(crate) marker: PhantomData<Q>,
}
impl<Q: QueryData> Query<'_, Q> {
    pub fn for_each(&mut self, mut visit: impl for<'a> FnMut(Entity, Q::Item<'a>)) -> usize {
        self.visit(Q::entities(&self.state), &mut visit)
    }
    /// Visits matching entities in ascending index/generation order.
    pub fn for_each_sorted(&mut self, mut visit: impl for<'a> FnMut(Entity, Q::Item<'a>)) -> usize {
        let mut entities = Q::entities(&self.state);
        entities.sort_unstable();
        self.visit(entities, &mut visit)
    }
    fn visit(
        &mut self,
        entities: Vec<Entity>,
        visit: &mut impl for<'a> FnMut(Entity, Q::Item<'a>),
    ) -> usize {
        let mut count = 0;
        for entity in entities {
            if let Some(row) = Q::get(&mut self.state, entity) {
                visit(entity, row);
                count += 1;
            }
        }
        count
    }
}
