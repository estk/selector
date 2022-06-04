use std::ops::Range;

use syn::parse::{Parse, ParseStream};
use syn::{Arm, Expr, ExprAwait, Ident, Pat, Token};

mod kw {
    syn::custom_keyword!(biased);
}

#[derive(Debug)]
pub struct Select {
    // span of `complete`, then expression after `=> ...`
    default: Option<Expr>,
    random: bool,
    futs: Vec<(ExprAwait, Option<Expr>, Range<usize>)>,
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
enum Partial {
    Default(Expr),
    Normal {
        futs: Vec<(ExprAwait, Option<Expr>)>,
        pat: Option<Pat>,
    },
    Match {
        futs: Vec<(ExprAwait, Option<Expr>)>,
    },
}

impl Parse for Select {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut select = Self {
            default: None,
            random: true,
            futs: vec![],
            arms: vec![],
        };

        if input.peek(kw::biased) {
            input.parse::<kw::biased>()?;
            input.parse::<Token![;]>()?;
        }

        while !input.is_empty() {
            let partial = if input.peek(Token![default]) {
                // `default`
                if select.default.is_some() {
                    return Err(input.error("multiple default cases found, only one allowed"));
                }
                input.parse::<Ident>()?;
                input.parse::<Token![=>]>()?;
                let expr = input.parse::<Expr>()?;
                Partial::Default(expr)
            } else {
                let pat = if input.peek2(Token![=]) {
                    // `<pat> = <fut1>.await [if <bool>], <fut2>.await [if <bool>], ... =>`
                    let pat = input.parse::<syn::Pat>()?;
                    input.parse::<Token![=]>()?;
                    Some(pat)
                } else {
                    // `<fut1>.await [if <bool>], <fut2>.await [if <bool>], ... =>`
                    None
                };
                let fut = input.parse::<ExprAwait>()?;
                let cond = if input.peek(Token![if]) {
                    input.parse::<Token![if]>()?;
                    Some(input.parse::<syn::Expr>()?)
                } else {
                    None
                };
                let mut futs = vec![(fut, cond)];
                while input.peek(Token![,]) {
                    input.parse::<Token![,]>()?;
                    let fut = input.parse::<ExprAwait>()?;
                    let cond = if input.peek(Token![if]) {
                        input.parse::<Token![if]>()?;
                        Some(input.parse::<syn::Expr>()?)
                    } else {
                        None
                    };
                    futs.push((fut, cond));
                }
                Partial::Normal { futs, pat }
            };
            if input.peek(Token![=>]) {
                // `=> <expr>`
                input.parse::<Token![=>]>()?;

                // todo(eas): ultra shorthand look for Token![_] then pattern against it
                // need to verify no existing pattern

                let expr = input.parse::<Expr>()?;
                // Commas after the expression are only optional if it's a `Block`
                // or it is the last branch in the `match`.
                let is_block = match expr {
                    Expr::Block(_) => true,
                    _ => false,
                };
                if is_block || input.is_empty() {
                    input.parse::<Option<Token![,]>>()?;
                } else {
                    input.parse::<Token![,]>()?;
                }

                if let Partial::Normal {
                    futs,
                    pat: Some(pat),
                } = partial
                {
                    select.arms.push((pat, Box::new(expr)));
                    let i = select.arms.len();
                    let iter = futs.drain(..).map(|(fut, cond)| (fut, cond, i..i + 1));
                    select.futs.extend(&mut iter)
                } else if let Partial::Default(expr) = partial {
                    select.default.replace(expr);
                } else {
                    panic!("unreachable")
                }
            } else if input.peek(syn::token::Brace) {
                if let Partial::Normal { pat: None, futs } = partial {
                    let arms_pb;
                    syn::braced!(arms_pb in input);
                    let mut arms: Vec<Arm> = vec![];
                    arms.push(arms_pb.parse::<Arm>()?);
                    while !arms_pb.is_empty() {
                        arms.push(arms_pb.parse::<Arm>()?);
                    }

                    input.parse::<Option<Token![,]>>()?;

                    let arm_iter = arms.drain(..).map(|a| (a.pat, a.body));
                    let i = select.arms.len();
                    select.arms.extend(arm_iter);
                    let j = select.arms.len();

                    let fut_iter = futs.drain(..).map(|(fut, cond)| (fut, cond, i..j));
                    select.futs.extend(fut_iter);
                } else {
                    panic!("A case may not have both a singular pattern and a match block")
                }
            } else {
                panic!("Invalid syntax, ExprAwait and condition must be followed by either a Brace or `=>`")
            }
        }

        Ok(select)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse() {
        let ts: Select = syn::parse_quote!(
            x = f1.await => x,
        );
        println!("ts1: {:#?}", ts);

        let ts: Select = syn::parse_quote!(
            _ = f1.await => x,
        );
        println!("ts2: {:#?}", ts);

        let ts: Select = syn::parse_quote!(
            _ = f1.await, f2.await => x,
        );
        println!("ts3: {:#?}", ts);

        let ts: Select = syn::parse_quote!(
            _ = f1.await, f2.await => x,
        );
        println!("ts4: {:#?}", ts);
        let ts: Select = syn::parse_quote!(
            f1.await {
                Ok(y) if y > 2 => y,
                Err(_) => 0,
                Ok(y) => y,
            },
        );
        println!("ts5: {:#?}", ts);
    }
}
