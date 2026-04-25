/// Key repeat settings.
///
/// Key repeat is configured using two values (both in milliseconds):
///
/// - The **`delay`** after the key has been pressed, after which key repeat events will be emitted.
/// - The **`period`** between successive key repeat events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct KeyRepeat {
    pub(crate) delay: u32,
    pub(crate) period: u32,
}

impl KeyRepeat {
    #[inline]
    pub const fn new(delay: u32, period: u32) -> Self {
        Self { delay, period }
    }

    #[inline]
    pub fn delay(&self) -> u32 {
        self.delay
    }

    #[inline]
    pub fn period(&self) -> u32 {
        self.period
    }
}
