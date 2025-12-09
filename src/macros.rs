/// ffi_enum! {}
macro_rules! ffi_enum {
    (
        $( #[$attrs:meta] )*
        $v:vis enum $name:ident: $native:ty {
            $(
                $( #[$variant_attrs:meta] )*
                $variant:ident = $value:expr
            ),+
            $(,)?
        }
    ) => {
        $( #[$attrs] )*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(transparent)]
        $v struct $name(pub(crate) $native);

        impl $name {
            $(
                $( #[$variant_attrs] )*
                $v const $variant: Self = Self($value);
            )+

            #[allow(dead_code, unreachable_patterns)]
            pub(crate) fn variant_name(&self) -> Option<&'static str> {
                match self {
                    $(
                        &Self::$variant => Some(stringify!($variant)),
                    )*
                    _ => None,
                }
            }

            #[allow(dead_code)]
            pub(crate) fn from_variant_name(name: &str) -> Option<Self> {
                match name {
                    $(
                        stringify!($variant) => Some(Self::$variant),
                    )*
                    _ => None,
                }
            }

            /// Returns the raw value wrapped by `self`.
            #[inline]
            $v const fn raw(self) -> $native {
                self.0
            }

            /// Creates a new value from its raw representation.
            #[inline]
            $v const fn from_raw(raw: $native) -> Self {
                Self(raw)
            }
        }
    };
}

macro_rules! bitvalue {
    ($type:ty) => {
        impl $crate::bits::BitValueImpl for $type {
            type __PrivateArray = [$crate::bits::Word;
                (Self::MAX.0 as usize + 1).div_ceil($crate::bits::Word::BITS as usize)];
            const __PRIVATE_ZERO: Self::__PrivateArray =
                [0; (Self::MAX.0 as usize + 1).div_ceil($crate::bits::Word::BITS as usize)];

            #[inline]
            fn from_index(index: usize) -> Self {
                Self(index as _)
            }
            #[inline]
            fn into_index(self) -> usize {
                self.0 as _
            }
        }
        impl $crate::bits::BitValue for $type {
            const MAX: Self = <Self>::MAX;
        }
    };
}
