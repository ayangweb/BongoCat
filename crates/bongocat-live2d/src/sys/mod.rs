#![allow(
    dead_code,
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals
)]

#[cfg(target_os = "macos")]
include!("macos.rs");

#[cfg(target_os = "windows")]
include!("windows.rs");
