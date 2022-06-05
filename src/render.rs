use std::ops::Range;

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::format_ident;
use quote::quote;
use syn::{Ident, Pat};

use crate::Select;
impl Select {
    fn render_conds(&self) -> Vec<proc_macro2::TokenStream> {
        let mut res = Vec::with_capacity(self.futs.len());
        for (i, (_, cond, _)) in self.futs.iter().enumerate() {
            // only include conditions that are defined
            // todo(eas): we could probably statically eliminate literals here too...
            if let Some(c) = cond {
                res.push(proc_macro2::TokenStream::from(quote! {
                    if #c {
                        let mask: __tokio_select_util::Mask = 1 << #i;
                        disabled |= mask;
                    }
                }));
            }
        }
        res
    }
    fn render_futs_tup(&self) -> proc_macro2::TokenStream {
        let futs = self.futs.iter().map(|(f, _, _)| f.base.clone());
        proc_macro2::TokenStream::from(quote! {
            (#(#futs,)*)
        })
    }

    fn mk_start(&self) -> proc_macro2::TokenStream {
        if self.random {
            proc_macro2::TokenStream::from(quote! {
                ::tokio::macros::support::thread_rng_n(FUTURES)
            })
        } else {
            proc_macro2::TokenStream::from(quote! {
                0
            })
        }
    }
    fn fut_ids(&self) -> Vec<u32> {
        self.futs
            .iter()
            .enumerate()
            .map(|(i, _)| i as u32)
            .collect()
    }
    fn mk_matches(&self) -> Vec<proc_macro2::TokenStream> {
        let rs: Vec<Range<usize>> = self.futs.iter().map(|(_, _, r)| r).cloned().collect();

        let mut res = vec![];
        for r in rs {
            let arms: &[(Pat, Box<syn::Expr>)] = &self.arms[r];
            let pats: Vec<Pat> = arms
                .iter()
                .map(|(p, _)| {
                    let mut pp = p.clone();
                    clean_pattern(&mut pp);
                    pp
                })
                .collect();
            res.push(proc_macro2::TokenStream::from(quote! {
                // The future returned a value, check if matches
                // the specified pattern.
                #[allow(unused_variables)]
                #[allow(unused_mut)]
                match &out {
                    #(
                        #pats => {}
                    )*
                    _ => continue,
                }
            }));
        }
        res
    }
}

pub(crate) fn render(parsed: Select) -> TokenStream {
    let span = Span::call_site();
    let fut_count = parsed.fut_count() as u32;

    let enum_item = declare_output_enum(parsed.fut_count(), span);
    let rendered_conds = parsed.render_conds();
    let rendered_futs_tup = parsed.render_futs_tup();
    let start = parsed.mk_start();
    let fut_ids = parsed.fut_ids();
    let fut_ids_pfx: Vec<_> = parsed
        .fut_ids()
        .iter()
        .map(|id| format_ident!("N{}", id))
        .collect();
    let matches = parsed.mk_matches();

    TokenStream::from(quote! { {
        #enum_item

        // `tokio::macros::support` is a public, but doc(hidden) module
        // including a re-export of all types needed by this macro.
        use ::tokio::macros::support::Future;
        use ::tokio::macros::support::Pin;
        use ::tokio::macros::support::Poll::{Ready, Pending};

        const FUTURES: u32 = #fut_count;

        let mut disabled: __tokio_select_util::Mask = Default::default();

        #(
            // First, invoke all the pre-conditions. For any that return true,
            // set the appropriate bit in `disabled`.
            #rendered_conds
        )*

        let mut output = {
            let mut futures = #rendered_futs_tup;
            ::tokio::macros::support::poll_fn(|cx| {
                let mut is_pending = false;

                // Choose a starting index to begin polling the futures at. In
                // practice, this will either be a pseudo-randomly generated
                // number by default, or the constant 0 if `biased;` is
                // supplied.
                let start = #start;

                // The inner polling loop
                for i in 0..FUTURES {

                    #[allow(clippy::modulo_one)]
                    let branch = {
                        (start + i) % FUTURES
                    };
                    match branch {
                        #[allow(unreachable_code)]
                        #(
                                #fut_ids => {
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
                                let fut = &mut futures.#fut_ids;

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

                                #matches
                                return Ready(__tokio_select_util::Out::#fut_ids_pfx(out))
                            }
                            )*
                            _ => unreachable!()
                    }
                }
                if is_pending {
                    Pending
                } else {
                    Ready(__tokio_select_util::Out::Disabled)
                }
            }).await
        };
        match output {
            _ => println!("matched")
        }
    }
    })
}

pub(crate) fn declare_output_enum(branches: usize, span: Span) -> proc_macro2::TokenStream {
    let variants = (0..branches)
        .map(|num| Ident::new(&format!("N{}", num), span))
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

    proc_macro2::TokenStream::from(quote! {
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
