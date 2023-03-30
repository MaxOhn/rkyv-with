#![doc = include_str!("../README.md")]

use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput};

mod archive_with;
mod deserialize_with;
mod util;

const ATTR: &str = "archive_with";

/// Derive macro to implement rkyv's `ArchiveWith` and `SerializeWith` traits.
///
/// See the crate root for more information.
#[proc_macro_derive(ArchiveWith, attributes(archive_with))]
pub fn archive_with(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match archive_with::derive(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Derive macro to implement rkyv's `DeserializeWith` trait.
///
/// See the crate root for more information.
#[proc_macro_derive(DeserializeWith, attributes(archive_with))]
pub fn deserialize_with(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match deserialize_with::derive(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
