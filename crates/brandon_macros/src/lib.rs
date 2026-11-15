//! Attribute macros for Brandon. Use them through the `brandon` crate.

use proc_macro::{Delimiter, TokenStream, TokenTree};

/// Does the item declare a struct with named fields (`struct Name { .. }`)?
fn has_named_fields(item: &TokenStream) -> bool {
    let mut is_struct = false;
    for token in item.clone() {
        match token {
            TokenTree::Ident(ident) if ident.to_string() == "struct" => is_struct = true,
            TokenTree::Ident(ident) if ident.to_string() == "enum" => return false,
            TokenTree::Group(group) if is_struct && group.delimiter() == Delimiter::Brace => {
                return true
            }
            _ => {}
        }
    }
    false
}

fn with_prefix(prefix: &str, item: TokenStream) -> TokenStream {
    let prefix: TokenStream = prefix
        .parse()
        .expect("the generated attributes are valid Rust");
    prefix.into_iter().chain(item).collect()
}

/// Makes a type usable as a behaviour in scene files.
///
/// It lets the type be copied, saved and loaded. Fields left out of a scene
/// file get their default value, so the type also needs `Default`:
///
/// ```ignore
/// #[behaviour]
/// #[derive(Default)]
/// struct Player {
///     speed: f32,
/// }
/// ```
///
/// You still write `impl Behaviour for Player` yourself, and register the
/// type with `Game::register`.
#[proc_macro_attribute]
pub fn behaviour(_args: TokenStream, item: TokenStream) -> TokenStream {
    let serde = if has_named_fields(&item) {
        "#[serde(crate = \"::brandon::serde\", default)]"
    } else {
        "#[serde(crate = \"::brandon::serde\")]"
    };
    with_prefix(
        &format!("#[derive(::core::clone::Clone, ::brandon::serde::Serialize, ::brandon::serde::Deserialize)] {serde}"),
        item,
    )
}

/// Makes a plain type (often an enum) usable as a field of a behaviour, so it
/// can be copied, saved and loaded.
///
/// ```ignore
/// #[data]
/// #[derive(Default)]
/// enum Kind {
///     #[default]
///     Star,
///     Rock,
/// }
/// ```
#[proc_macro_attribute]
pub fn data(_args: TokenStream, item: TokenStream) -> TokenStream {
    with_prefix(
        "#[derive(::core::clone::Clone, ::brandon::serde::Serialize, ::brandon::serde::Deserialize)] \
         #[serde(crate = \"::brandon::serde\")]",
        item,
    )
}
