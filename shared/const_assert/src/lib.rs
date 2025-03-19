//! Provides the `const_assert!()` macro which allows compile-time assertions

#![no_std]

#[macro_export]
macro_rules! const_assert {
    ($x:expr) => {
        const _: [(); 0 - !($x) as usize] = [];
    };
}
