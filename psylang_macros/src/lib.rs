use std::collections::HashSet as Set;

use felt_sized::derive_felt_sized;
use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use state_init::derive_state_init;
use syn::{
    fold::{self, Fold},
    parse::{Parse, ParseStream, Result},
    parse_macro_input, parse_quote,
    punctuated::Punctuated,
    BinOp, DeriveInput, Expr, Ident, ItemFn, ItemImpl, Local, Pat, Stmt, Token,
};
mod felt_sized;
mod gm1;
mod heap2;
mod state_init;
use gm1::RewriterVisitor;
use psy_vm::dpn::ops::context_trait::FeltSized;
/// Parses a list of variable names separated by commas.
///
///     a, b, c
///
/// This is how the compiler passes in arguments to our attribute -- it is
/// everything inside the delimiters after the attribute name.
///
///     #[trace_var(a, b, c)]
///                 ^^^^^^^
struct Args {
    vars: Set<Ident>,
}

impl Parse for Args {
    fn parse(input: ParseStream) -> Result<Self> {
        let vars = Punctuated::<Ident, Token![,]>::parse_terminated(input)?;
        Ok(Args {
            vars: vars.into_iter().collect(),
        })
    }
}

impl Args {
    /// Determines whether the given `Expr` is a path referring to one of the
    /// variables we intend to print. Expressions are used as the left-hand side
    /// of the assignment operator.
    fn should_print_expr(&self, e: &Expr) -> bool {
        match *e {
            Expr::Path(ref e) => {
                if e.path.leading_colon.is_some() {
                    false
                } else if e.path.segments.len() != 1 {
                    false
                } else {
                    let first = e.path.segments.first().unwrap();
                    first.arguments.is_empty()
                }
            }
            _ => false,
        }
    }

    /// Determines whether the given `Pat` is an identifier equal to one of the
    /// variables we intend to print. Patterns are used as the left-hand side of
    /// a `let` binding.
    fn should_print_pat(&self, p: &Pat) -> bool {
        match p {
            Pat::Ident(ref p) => true,
            _ => false,
        }
    }

    /// Produces an expression that assigns the right-hand side to the left-hand
    /// side and then prints the value.
    ///
    ///     // Before
    ///     VAR = INIT
    ///
    ///     // After
    ///     { VAR = INIT; println!("VAR = {:?}", VAR); }
    fn assign_and_print(&mut self, left: Expr, op: &dyn ToTokens, right: Expr) -> Expr {
        let right = fold::fold_expr(self, right);
        parse_quote!({
            #left #op #right;
            println!(concat!(stringify!(#left), " = {:?}"), #left);
        })
    }
    fn assign_and_print_bin(&mut self, left: Expr, op: &BinOp, right: Expr) -> Expr {
        let right = fold::fold_expr(self, right);
        // replace with ctx.op_<op>(left, right)
        /*
        match  op {
            BinOp::Add(_) => todo!(),
            BinOp::Sub(_) => todo!(),
            BinOp::Mul(_) => todo!(),
            BinOp::Div(_) => todo!(),
            BinOp::Rem(_) => todo!(),
            BinOp::And(_) => todo!(),
            BinOp::Or(_) => todo!(),
            BinOp::BitXor(_) => todo!(),
            BinOp::BitAnd(_) => todo!(),
            BinOp::BitOr(_) => todo!(),
            BinOp::Shl(_) => todo!(),
            BinOp::Shr(_) => todo!(),
            BinOp::Eq(_) => todo!(),
            BinOp::Lt(_) => todo!(),
            BinOp::Le(_) => todo!(),
            BinOp::Ne(_) => todo!(),
            BinOp::Ge(_) => todo!(),
            BinOp::Gt(_) => todo!(),
            BinOp::AddAssign(_) => todo!(),
            BinOp::SubAssign(_) => todo!(),
            BinOp::MulAssign(_) => todo!(),
            BinOp::DivAssign(_) => todo!(),
            BinOp::RemAssign(_) => todo!(),
            BinOp::BitXorAssign(_) => todo!(),
            BinOp::BitAndAssign(_) => todo!(),
            BinOp::BitOrAssign(_) => todo!(),
            BinOp::ShlAssign(_) => todo!(),
            BinOp::ShrAssign(_) => todo!(),
            _ => todo!(),
        }*/

        parse_quote!({
            #left #op #right;
            println!(concat!(stringify!(#left), " = {:?}"), #left);
        })
    }

    /// Produces a let-binding that assigns the right-hand side to the left-hand
    /// side and then prints the value.
    ///
    ///     // Before
    ///     let VAR = INIT;
    ///
    ///     // After
    ///     let VAR = { let VAR = INIT; println!("VAR = {:?}", VAR); VAR };
    fn let_and_print(&mut self, local: Local) -> Stmt {
        let Local { pat, init, .. } = local;
        let init = self.fold_expr(*init.unwrap().expr);
        let ident = match pat {
            Pat::Ident(ref p) => &p.ident,
            _ => unreachable!(),
        };
        parse_quote! {
            let #pat = {
                #[allow(unused_mut)]
                let #pat = #init;
                println!(concat!(stringify!(#ident), " = {:?}"), #ident);
                #ident
            };
        }
    }
}

/// The `Fold` trait is a way to traverse an owned syntax tree and replace some
/// of its nodes.
///
/// Syn provides two other syntax tree traversal traits: `Visit` which walks a
/// shared borrow of a syntax tree, and `VisitMut` which walks an exclusive
/// borrow of a syntax tree and can mutate it in place.
///
/// All three traits have a method corresponding to each type of node in Syn's
/// syntax tree. All of these methods have default no-op implementations that
/// simply recurse on any child nodes. We can override only those methods for
/// which we want non-default behavior. In this case the traversal needs to
/// transform `Expr` and `Stmt` nodes.
impl Fold for Args {
    fn fold_expr(&mut self, e: Expr) -> Expr {
        match e {
            Expr::Assign(e) => {
                if self.should_print_expr(&e.left) {
                    self.assign_and_print(*e.left, &e.eq_token, *e.right)
                } else {
                    Expr::Assign(fold::fold_expr_assign(self, e))
                }
            }
            Expr::Binary(e) if is_assign_op(e.op) => {
                if self.should_print_expr(&e.left) {
                    self.assign_and_print(*e.left, &e.op, *e.right)
                } else {
                    Expr::Binary(fold::fold_expr_binary(self, e))
                }
            }
            _ => fold::fold_expr(self, e),
        }
    }

    fn fold_stmt(&mut self, s: Stmt) -> Stmt {
        match s {
            Stmt::Local(s) => {
                if s.init.is_some() && self.should_print_pat(&s.pat) {
                    self.let_and_print(s)
                } else {
                    Stmt::Local(fold::fold_local(self, s))
                }
            }
            _ => fold::fold_stmt(self, s),
        }
    }
}

fn is_assign_op(op: BinOp) -> bool {
    match op {
        BinOp::AddAssign(_)
        | BinOp::SubAssign(_)
        | BinOp::MulAssign(_)
        | BinOp::DivAssign(_)
        | BinOp::RemAssign(_)
        | BinOp::BitXorAssign(_)
        | BinOp::BitAndAssign(_)
        | BinOp::BitOrAssign(_)
        | BinOp::ShlAssign(_)
        | BinOp::ShrAssign(_) => true,
        _ => false,
    }
}

/// Attribute to print the value of the given variables each time they are
/// reassigned.
///
/// # Example
///
/// ```
/// #[trace_var(p, n)]
/// fn factorial(mut n: u64) -> u64 {
///     let mut p = 1;
///     while n > 1 {
///         p *= n;
///         n -= 1;
///     }
///     p
/// }
/// ```
#[proc_macro_attribute]
pub fn trace_var(args: TokenStream, input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as ItemFn);

    // Parse the list of variables the user wanted to print.
    let mut args = parse_macro_input!(args as Args);

    // Use a syntax tree traversal to transform the function body.
    let output = args.fold_item_fn(input);

    // Hand the resulting function body back to the compiler.
    TokenStream::from(quote!(#output))
}

#[proc_macro_attribute]
pub fn qcontract(args: TokenStream, input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as ItemImpl);

    // Parse the list of variables the user wanted to print.
    let mut vs = RewriterVisitor {};

    // Use a syntax tree traversal to transform the function body.
    let output = vs.fold_item_impl(input);

    // Hand the resulting function body back to the compiler.
    TokenStream::from(quote!(#output))
}

#[proc_macro_attribute]
pub fn show_streams(attr: TokenStream, item: TokenStream) -> TokenStream {
    println!("attr: \"{attr}\"");
    println!("item: \"{item}\"");
    let item_old = item.clone();
    let input = parse_macro_input!(item as DeriveInput);
    println!("input: {:#?}", input);
    "fn test_function(x: FeltOp, y: FeltOp) -> FeltOp {
        x
    }"
    .parse()
    .unwrap()
}

#[proc_macro_derive(QStateInitializable)]
pub fn derive_state(input: TokenStream) -> TokenStream {
    derive_state_init(input.into()).into()
}

#[proc_macro_derive(FeltSized)]
pub fn derive_feltsz(input: TokenStream) -> TokenStream {
    derive_felt_sized(input.into()).into()
}
