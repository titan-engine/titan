use super::{Component, Entity, InsertError, World};

/// Components inserted together, in tuple order.
///
/// A component, `()`, or a tuple of up to twelve bundles can be used as a
/// bundle. Tuples may be nested. Repeated component types replace earlier
/// values, so the final value wins. Existing components outside the bundle
/// remain unchanged.
///
/// This trait is sealed; use tuples or a function returning `impl Bundle` for
/// reusable entity constructors.
pub trait Bundle: private::Sealed + Send + 'static {
    #[doc(hidden)]
    fn insert_into(self, world: &mut World, entity: Entity) -> Result<(), InsertError>;
}

mod private {
    pub trait Sealed {}
}

impl<T: Component> private::Sealed for T {}

impl<T: Component> Bundle for T {
    fn insert_into(self, world: &mut World, entity: Entity) -> Result<(), InsertError> {
        world.insert(entity, self).map(|_| ())
    }
}

impl private::Sealed for () {}

impl Bundle for () {
    fn insert_into(self, _world: &mut World, _entity: Entity) -> Result<(), InsertError> {
        Ok(())
    }
}

macro_rules! impl_bundle {
    ($($component:ident),+) => {
        impl<$($component: Bundle),+> private::Sealed for ($($component,)+) {}

        impl<$($component: Bundle),+> Bundle for ($($component,)+) {
            #[allow(non_snake_case)]
            fn insert_into(self, world: &mut World, entity: Entity) -> Result<(), InsertError> {
                let ($($component,)+) = self;
                $( $component.insert_into(world, entity)?; )+
                Ok(())
            }
        }
    };
}

impl_bundle!(A);
impl_bundle!(A, B);
impl_bundle!(A, B, C);
impl_bundle!(A, B, C, D);
impl_bundle!(A, B, C, D, E);
impl_bundle!(A, B, C, D, E, F);
impl_bundle!(A, B, C, D, E, F, G);
impl_bundle!(A, B, C, D, E, F, G, H);
impl_bundle!(A, B, C, D, E, F, G, H, I);
impl_bundle!(A, B, C, D, E, F, G, H, I, J);
impl_bundle!(A, B, C, D, E, F, G, H, I, J, K);
impl_bundle!(A, B, C, D, E, F, G, H, I, J, K, L);
