/// Define a plain-data component with defaults, in one declaration.
///
/// ```
/// use lunaris_ecs::component;
/// component! {
///     /// Position of an entity.
///     pub struct Pos { pub x: f32 = 0.0, pub y: f32 = 0.0 }
/// }
/// assert_eq!(Pos::default().x, 0.0);
/// ```
///
/// Field types must be `f32 f64 i32 u32 i64 u64 bool Sym Uid` or a fixed array of one of those (`[f32; 3]`). Only doc comments may
/// precede `struct`: they become the component's description in the `schema` tool.
/// The struct is `#[repr(C)]`, `Copy` and gets a schema automatically.
#[macro_export]
macro_rules! component {
    (
        $(#[doc = $doc:literal])*
        $vis:vis struct $name:ident { $($fvis:vis $f:ident : $t:ty = $def:expr),* $(,)? }
    ) => {
        $(#[doc = $doc])*
        #[repr(C)]
        #[derive(Clone, Copy, Debug, PartialEq)]
        $vis struct $name { $($fvis $f: $t),* }

        impl ::core::default::Default for $name {
            fn default() -> Self { Self { $($f: $def),* } }
        }

        impl $crate::Component for $name {
            #[allow(unused_mut, unused_variables)]
            fn spec() -> $crate::NativeSpec {
                let d = <$name as ::core::default::Default>::default();
                let mut default_bytes = ::std::vec![0u8; ::core::mem::size_of::<$name>()];
                $(
                    // SAFETY: copies exactly the bytes of one field into its slot; padding is never read.
                    unsafe {
                        ::core::ptr::copy_nonoverlapping(
                            ::core::ptr::addr_of!(d.$f).cast::<u8>(),
                            default_bytes.as_mut_ptr().add(::core::mem::offset_of!($name, $f)),
                            ::core::mem::size_of::<$t>(),
                        );
                    }
                )*
                $crate::NativeSpec {
                    name: ::core::stringify!($name),
                    size: ::core::mem::size_of::<$name>(),
                    align: ::core::mem::align_of::<$name>(),
                    fields: ::std::vec![
                        $((
                            ::core::stringify!($f),
                            <$t as $crate::FieldKind>::TYPE,
                            <$t as $crate::FieldKind>::COUNT,
                            ::core::mem::offset_of!($name, $f),
                        )),*
                    ],
                    default_bytes,
                    // the doc comment, which is what the `schema` tool tells an AI about the component (lines joined by `world.rs`)
                    doc: ::core::concat!($($doc, "
"),*),
                }
            }
        }
    };
}
