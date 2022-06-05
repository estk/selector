mod parse;
mod render;
use std::ops::Range;

use proc_macro::TokenStream;
use syn::{Expr, ExprAwait, Pat};

/// The `select!` macro.
#[proc_macro]
pub fn select(input: TokenStream) -> TokenStream {
    let parsed = syn::parse_macro_input!(input as Select);
    render::render(parsed)
}

#[derive(Debug)]
pub(crate) struct Select {
    // span of `complete`, then expression after `=> ...`
    default: Option<Expr>,
    random: bool,
    // (Future, EnabledCondition, ArmsToMatch)
    futs: Vec<(ExprAwait, Option<Expr>, Range<usize>)>,
    // (Pattern, Body)
    arms: Vec<(Pat, Box<Expr>)>,
}
impl Select {
    pub fn fut_count(&self) -> usize {
        self.futs.len()
    }
    pub fn case_count(&self) -> usize {
        if self.default.is_some() {
            self.futs.len() + 1
        } else {
            self.futs.len()
        }
    }
}
