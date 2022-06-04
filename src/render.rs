use proc_macro::{TokenStream, TokenTree};
use proc_macro2::Span;
use quote::quote;
use syn::{parse_quote, Ident};

use crate::Select;

pub(crate) fn render(parsed: Select) -> TokenStream {
    let span = Span::call_site();

    let enum_item = declare_output_enum(parsed.futs.len(), span);

    // bind non-`Ident` future exprs w/ `let`
    let mut future_let_bindings = Vec::with_capacity(parsed.normal_fut_exprs.len());
    let bound_future_names: Vec<_> = parsed
        .normal_fut_exprs
        .into_iter()
        .zip(variant_names.iter())
        .map(|(expr, variant_name)| {
            match expr {
                syn::Expr::Path(path) => {
                    // Don't bind futures that are already a path.
                    // This prevents creating redundant stack space
                    // for them.
                    // Passing Futures by path requires those Futures to implement Unpin.
                    // We check for this condition here in order to be able to
                    // safely use Pin::new_unchecked(&mut #path) later on.
                    future_let_bindings.push(quote! {
                        __futures_crate::async_await::assert_fused_future(&#path);
                        __futures_crate::async_await::assert_unpin(&#path);
                    });
                    path
                }
                _ => {
                    // Bind and pin the resulting Future on the stack. This is
                    // necessary to support direct select! calls on !Unpin
                    // Futures. The Future is not explicitly pinned here with
                    // a Pin call, but assumed as pinned. The actual Pin is
                    // created inside the poll() function below to defer the
                    // creation of the temporary pointer, which would otherwise
                    // increase the size of the generated Future.
                    // Safety: This is safe since the lifetime of the Future
                    // is totally constraint to the lifetime of the select!
                    // expression, and the Future can't get moved inside it
                    // (it is shadowed).
                    future_let_bindings.push(quote! {
                        let mut #variant_name = #expr;
                    });
                    parse_quote! { #variant_name }
                }
            }
        })
        .collect();

    // For each future, make an `&mut dyn FnMut(&mut Context<'_>) -> Option<Poll<__PrivResult<...>>`
    // to use for polling that individual future. These will then be put in an array.
    let poll_functions = bound_future_names.iter().zip(variant_names.iter()).map(
        |(bound_future_name, variant_name)| {
            // Below we lazily create the Pin on the Future below.
            // This is done in order to avoid allocating memory in the generator
            // for the Pin variable.
            // Safety: This is safe because one of the following condition applies:
            // 1. The Future is passed by the caller by name, and we assert that
            //    it implements Unpin.
            // 2. The Future is created in scope of the select! function and will
            //    not be moved for the duration of it. It is thereby stack-pinned
            quote! {
                let mut #variant_name = |__cx: &mut __futures_crate::task::Context<'_>| {
                    let mut #bound_future_name = unsafe {
                        __futures_crate::Pin::new_unchecked(&mut #bound_future_name)
                    };
                    if __futures_crate::future::FusedFuture::is_terminated(&#bound_future_name) {
                        __futures_crate::None
                    } else {
                        __futures_crate::Some(__futures_crate::future::FutureExt::poll_unpin(
                            &mut #bound_future_name,
                            __cx,
                        ).map(#enum_ident::#variant_name))
                    }
                };
                let #variant_name: &mut dyn FnMut(
                    &mut __futures_crate::task::Context<'_>
                ) -> __futures_crate::Option<__futures_crate::task::Poll<_>> = &mut #variant_name;
            }
        },
    );

    let none_polled = if parsed.complete.is_some() {
        quote! {
            __futures_crate::task::Poll::Ready(#enum_ident::Complete)
        }
    } else {
        quote! {
            panic!("all futures in select! were completed,\
                    but no `complete =>` handler was provided")
        }
    };

    let branches = parsed
        .normal_fut_handlers
        .into_iter()
        .zip(variant_names.iter())
        .map(|((pat, expr), variant_name)| {
            quote! {
                #enum_ident::#variant_name(#pat) => { #expr },
            }
        });
    let branches = quote! { #( #branches )* };

    let await_select_fut = if parsed.default.is_some() {
        // For select! with default this returns the Poll result
        quote! {
            __poll_fn(&mut __futures_crate::task::Context::from_waker(
                __futures_crate::task::noop_waker_ref()
            ))
        }
    } else {
        quote! {
            __futures_crate::future::poll_fn(__poll_fn).await
        }
    };

    let execute_result_expr = if let Some(default_expr) = &parsed.default {
        // For select! with default __select_result is a Poll, otherwise not
        quote! {
            match __select_result {
                __futures_crate::task::Poll::Ready(result) => match result {
                    #branches
                },
                _ => #default_expr
            }
        }
    } else {
        quote! {
            match __select_result {
                #branches
            }
        }
    };

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
