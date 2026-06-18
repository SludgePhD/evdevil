#[macro_use]
#[cfg(feature = "log")]
mod log {
    macro_rules! trace {
        ($($t:tt)+) => {
            ::log::trace!($($t)+)
        };
    }
    macro_rules! debug {
        ($($t:tt)+) => {
            ::log::debug!($($t)+)
        };
    }
    #[expect(unused_macros)] // currently unused
    macro_rules! info {
        ($($t:tt)+) => {
            ::log::info!($($t)+)
        };
    }
    macro_rules! warn {
        ($($t:tt)+) => {
            ::log::warn!($($t)+)
        };
    }
    macro_rules! error {
        ($($t:tt)+) => {
            ::log::error!($($t)+)
        };
    }
}

#[macro_use]
#[cfg(not(feature = "log"))]
mod log {
    macro_rules! trace {
        ($($t:tt)+) => {{
            ::core::format_args!($($t)+);
        }};
    }
    macro_rules! debug {
        ($($t:tt)+) => {{
            ::core::format_args!($($t)+);
        }};
    }
    #[expect(unused_macros)] // currently unused
    macro_rules! info {
        ($($t:tt)+) => {{
            ::core::format_args!($($t)+);
        }};
    }
    macro_rules! warn {
        ($($t:tt)+) => {{
            ::core::format_args!($($t)+);
        }};
    }
    macro_rules! error {
        ($($t:tt)+) => {{
            ::core::format_args!($($t)+);
        }};
    }
}
