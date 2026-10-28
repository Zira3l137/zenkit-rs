/// Declares a fieldless enum read from a raw integer.
///
/// The engine never validates these values and mods occasionally store odd ones, so an unknown
/// value logs a warning and falls back to the given default instead of failing the whole file.
#[macro_export]
macro_rules! raw_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident: $repr:ty = $default:ident {
            $($(#[$variant_meta:meta])* $variant:ident = $value:literal,)*
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr($repr)]
        pub enum $name {
            $($(#[$variant_meta])* $variant = $value,)*
        }

        impl Default for $name {
            fn default() -> Self {
                Self::$default
            }
        }

        impl $name {
            #[allow(dead_code)]
            pub(crate) fn from_raw(raw: u32) -> Self {
                match raw {
                    $($value => Self::$variant,)*
                    _ => {
                        $crate::__private::tracing::warn!(concat!("unknown ", stringify!($name), " {}, using {:?}"), raw, Self::$default);
                        Self::$default
                    }
                }
            }
        }
    };
}
