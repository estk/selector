use proc_macro::{TokenStream, TokenTree};
use proc_macro2::Span;
use quote::quote;
use syn::{parse_quote, Ident};

use crate::Select;

pub(crate) fn render(parsed: Select) -> TokenStream {
    let span = Span::call_site();
    let fut_count = parsed.fut_count();
    let case_count = parsed.case_count();

    let enum_item = declare_output_enum(parsed.fut_count(), span);

    TokenStream::from(quote! { {
        #enum_item

        // `tokio::macros::support` is a public, but doc(hidden) module
        // including a re-export of all types needed by this macro.
        use $crate::macros::support::Future;
        use $crate::macros::support::Pin;
        use $crate::macros::support::Poll::{Ready, Pending};

        const FUTURES: u32 = select.branches;

        let mut disabled: __tokio_select_util::Mask = Default::default();

        // First, invoke all the pre-conditions. For any that return true,
        // set the appropriate bit in `disabled`.
        for (i, c) in parsed.conds.enumerate() {
                // todo(eas): idx right?
            let mask: __tokio_select_util::Mask = 1 << i;
            disabled |= mask;
        }

        let mut output = {
            let futures = ( #( #fut, )+ );
            $crate::macros::support::poll_fn(|cx| {
                let mut is_pending = false;

                // Choose a starting index to begin polling the futures at. In
                // practice, this will either be a pseudo-randomly generated
                // number by default, or the constant 0 if `biased;` is
                // supplied.
                let start = if parsed.biased {
                    0;
                } else {
                    $crate::macros::support::thread_rng_n(FUTURES)
                };

                for i in 0..FUTURES {
                    let branch;
                    #[allow(clippy::modulo_one)]
                    {
                        branch = (start + i) % FUTURES;
                    }
                    match branch {
                        #[allow(unreachable_code)]
                        #(
                             #fut_idx => {
                                // First, if the future has previously been
                                // disabled, do not poll it again. This is done
                                // by checking the associated bit in the
                                // `disabled` bit field.
                                let mask = 1 << branch;

                                if disabled & mask == mask {
                                    // The future has been disabled.
                                    continue;
                                }

                                // Extract the future for this branch from the
                                // tuple
                                let fut = &mut futures.#fut_idx;

                                // Safety: future is stored on the stack above
                                // and never moved.
                                let mut fut = unsafe { Pin::new_unchecked(fut) };

                                // Try polling it
                                let out = match Future::poll(fut, cx) {
                                    Ready(out) => out,
                                    Pending => {
                                        // Track that at least one future is
                                        // still pending and continue polling.
                                        is_pending = true;
                                        continue;
                                    }
                                };

                                // Disable the future from future polling.
                                disabled |= mask;

                                // The future returned a value, check if matches
                                // the specified pattern.
                                #[allow(unused_variables)]
                                #[allow(unused_mut)]
                                match &out {
                                    $crate::select_priv_clean_pattern!(#bind) => {}
                                    _ => continue,
                                }

                                // The select is complete, return the value
                                return Ready(format_ident!("__tokio_select_util::Out::_{}({})", #fut_idx, out))
                            }
                         )*
                        }
                    }
            });
        };

        #output
    } })
}

pub(crate) fn declare_output_enum(branches: usize, span: Span) -> TokenStream {
    let variants = (0..branches)
        .map(|num| Ident::new(&format!("_{}", num), span))
        .collect::<Vec<_>>();

    // Use a bitfield to track which futures completed
    let mask = Ident::new(
        if branches <= 8 {
            "u8"
        } else if branches <= 16 {
            "u16"
        } else if branches <= 32 {
            "u32"
        } else if branches <= 64 {
            "u64"
        } else {
            panic!("up to 64 branches supported");
        },
        span,
    );

    TokenStream::from(quote! {
        mod __tokio_select_util {
            pub(super) enum Out<#( #variants ),*> {
                #( #variants(#variants), )*
                // Include a `Disabled` variant signifying that all select branches
                // failed to resolve.
                Disabled,
            }

            pub(super) type Mask = #mask;
        }
    })
}

pub(crate) fn clean_pattern_macro(input: TokenStream) -> TokenStream {
    // If this isn't a pattern, we return the token stream as-is. The select!
    // macro is using it in a location requiring a pattern, so an error will be
    // emitted there.
    let mut input: syn::Pat = match syn::parse(input.clone()) {
        Ok(it) => it,
        Err(_) => return input,
    };

    clean_pattern(&mut input);
    quote::ToTokens::into_token_stream(input).into()
}

// Removes any occurrences of ref or mut in the provided pattern.
fn clean_pattern(pat: &mut syn::Pat) {
    match pat {
        syn::Pat::Box(_box) => {}
        syn::Pat::Lit(_literal) => {}
        syn::Pat::Macro(_macro) => {}
        syn::Pat::Path(_path) => {}
        syn::Pat::Range(_range) => {}
        syn::Pat::Rest(_rest) => {}
        syn::Pat::Verbatim(_tokens) => {}
        syn::Pat::Wild(_underscore) => {}
        syn::Pat::Ident(ident) => {
            ident.by_ref = None;
            ident.mutability = None;
            if let Some((_at, pat)) = &mut ident.subpat {
                clean_pattern(&mut *pat);
            }
        }
        syn::Pat::Or(or) => {
            for case in or.cases.iter_mut() {
                clean_pattern(case);
            }
        }
        syn::Pat::Slice(slice) => {
            for elem in slice.elems.iter_mut() {
                clean_pattern(elem);
            }
        }
        syn::Pat::Struct(struct_pat) => {
            for field in struct_pat.fields.iter_mut() {
                clean_pattern(&mut field.pat);
            }
        }
        syn::Pat::Tuple(tuple) => {
            for elem in tuple.elems.iter_mut() {
                clean_pattern(elem);
            }
        }
        syn::Pat::TupleStruct(tuple) => {
            for elem in tuple.pat.elems.iter_mut() {
                clean_pattern(elem);
            }
        }
        syn::Pat::Reference(reference) => {
            reference.mutability = None;
            clean_pattern(&mut *reference.pat);
        }
        syn::Pat::Type(type_pat) => {
            clean_pattern(&mut *type_pat.pat);
        }
        _ => {}
    }
}
