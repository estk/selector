use syn::parse::{Parse, ParseStream};
use syn::{Arm, Expr, ExprAwait, Ident, Pat, Token};

mod kw {
    syn::custom_keyword!(biased);
}

#[derive(Debug)]
pub struct Select {
    // span of `complete`, then expression after `=> ...`
    default: Option<Expr>,
    cases: Vec<CaseKind>,
    random: bool,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
enum CaseKind {
    Default(Expr),
    Normal {
        pat: Option<Pat>,
        futs: Vec<(ExprAwait, Option<Expr>)>,
        body: Option<Expr>,
    },
    Match {
        futs: Vec<(ExprAwait, Option<Expr>)>,
        arms: Vec<Arm>,
    },
}

impl Parse for Select {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut select = Self {
            default: None,
            cases: vec![],
            random: true,
        };

        if input.peek(kw::biased) {
            input.parse::<kw::biased>()?;
            input.parse::<Token![;]>()?;
        }

        while !input.is_empty() {
            let mut case_kind = if input.peek(Token![default]) {
                // `default`
                if select.default.is_some() {
                    return Err(input.error("multiple default cases found, only one allowed"));
                }
                input.parse::<Ident>()?;
                input.parse::<Token![=>]>()?;
                let expr = input.parse::<Expr>()?;
                CaseKind::Default(expr)
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
                CaseKind::Normal {
                    pat,
                    futs,
                    body: None,
                }
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
                if let CaseKind::Normal { body, .. } = &mut case_kind {
                    body.replace(expr);
                } else {
                    panic!("unreachable")
                }
            } else if input.peek(syn::token::Brace) {
                if let CaseKind::Normal {
                    pat: None,
                    futs,
                    body: None,
                } = case_kind
                {
                    let arms_pb;
                    syn::braced!(arms_pb in input);
                    let mut arms: Vec<Arm> = vec![];
                    arms.push(arms_pb.parse::<Arm>()?);
                    while !arms_pb.is_empty() {
                        arms.push(arms_pb.parse::<Arm>()?);
                    }

                    input.parse::<Option<Token![,]>>()?;
                    case_kind = CaseKind::Match { futs, arms }
                } else {
                    panic!("A case may not have both a singular pattern and a match block")
                }
            } else {
                panic!("Invalid syntax, ExprAwait and condition must be followed by either a Brace or `=>`")
            }

            match case_kind {
                CaseKind::Default(expr) => select.default = Some(expr),
                _ => select.cases.push(case_kind),
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
