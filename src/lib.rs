mod parse;
mod render;
use parse::Select;
use proc_macro::TokenStream;

/// The `select!` macro.
#[proc_macro]
pub(crate) fn select(input: TokenStream) -> TokenStream {
    let parsed = syn::parse_macro_input!(input as Select);
    render::render(parsed)
}
