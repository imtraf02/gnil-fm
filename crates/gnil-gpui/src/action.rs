use std::any::Any;

pub use no_action::{NoAction, is_no_action};

/// Defines unit structs that can be bound and dispatched as actions.
#[macro_export]
macro_rules! actions {
    ($namespace:path, [ $( $(#[$attr:meta])* $name:ident),* $(,)? ]) => {
        $(
            $(#[$attr])*
            #[allow(dead_code)]
            #[derive(Clone, Debug, Default, PartialEq)]
            pub struct $name;

            impl $crate::Action for $name {
                fn boxed_clone(&self) -> Box<dyn $crate::Action> {
                    Box::new(self.clone())
                }

                fn partial_eq(&self, action: &dyn $crate::Action) -> bool {
                    action.as_any().downcast_ref::<Self>() == Some(self)
                }

                fn name(&self) -> &'static str {
                    concat!(stringify!($namespace), "::", stringify!($name))
                }

                fn name_for_type() -> &'static str {
                    concat!(stringify!($namespace), "::", stringify!($name))
                }
            }
        )*
    };
    ([ $( $(#[$attr:meta])* $name:ident),* $(,)? ]) => {
        $(
            $(#[$attr])*
            #[allow(dead_code)]
            #[derive(Clone, Debug, Default, PartialEq)]
            pub struct $name;

            impl $crate::Action for $name {
                fn boxed_clone(&self) -> Box<dyn $crate::Action> {
                    Box::new(self.clone())
                }

                fn partial_eq(&self, action: &dyn $crate::Action) -> bool {
                    action.as_any().downcast_ref::<Self>() == Some(self)
                }

                fn name(&self) -> &'static str {
                    stringify!($name)
                }

                fn name_for_type() -> &'static str {
                    stringify!($name)
                }
            }
        )*
    };
}

/// A statically constructed keyboard or command action.
pub trait Action: Any + Send {
    /// Clone the action into a type-erased box.
    fn boxed_clone(&self) -> Box<dyn Action>;

    /// Compare two type-erased actions.
    fn partial_eq(&self, action: &dyn Action) -> bool;

    /// Return the stable action name.
    fn name(&self) -> &'static str;

    /// Return the stable name for this action type.
    fn name_for_type() -> &'static str
    where
        Self: Sized;
}

impl std::fmt::Debug for dyn Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("dyn Action")
            .field("name", &self.name())
            .finish()
    }
}

impl dyn Action {
    /// Return this action as [`Any`] for downcasting.
    pub fn as_any(&self) -> &dyn Any {
        self
    }
}

mod no_action {
    use crate as gpui;
    use std::any::Any as _;

    actions!(
        zed,
        [
            /// Removes the highest-precedence matching key binding.
            NoAction
        ]
    );

    /// Returns whether an action removes a matching key binding.
    pub fn is_no_action(action: &dyn gpui::Action) -> bool {
        action.as_any().type_id() == NoAction.type_id()
    }
}
