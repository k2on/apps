//! §1.4 `#[derive(Node)]`: a struct is a `Tuple` of its fields in
//! declaration order; an enum is a `Sum` whose variant `i` is the tuple of
//! the `i`-th variant's fields (none for a unit variant). Each field is
//! encoded by its own type's `Node`, so a type is the composition of its
//! parts' and no walk is written by hand. A type that contains itself is
//! `Named` by its identifier where it recurs (`ark5::ir::named`), which is
//! the only way the five constructors can say a recursive type.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields};

/// The field types, a pattern binding them `f0, f1, …`, and the
/// constructor from `take`s in order.
fn parts(fields: &Fields, path: TokenStream) -> (Vec<&syn::Type>, TokenStream, TokenStream) {
    let tys: Vec<_> = fields.iter().map(|f| &f.ty).collect();
    let vars: Vec<_> = (0..tys.len()).map(|i| quote::format_ident!("f{i}")).collect();
    let take = quote!(::ark5::ir::Node::take(r)?);
    match fields {
        Fields::Named(n) => {
            let names: Vec<_> = n.named.iter().map(|f| f.ident.as_ref().unwrap()).collect();
            let takes = names.iter().map(|_| &take);
            (tys, quote!(#path { #(#names: #vars),* }), quote!(#path { #(#names: #takes),* }))
        }
        Fields::Unnamed(_) => {
            let takes = vars.iter().map(|_| &take);
            (tys, quote!(#path ( #(#vars),* )), quote!(#path ( #(#takes),* )))
        }
        Fields::Unit => (tys, path.clone(), path),
    }
}

pub fn node(input: DeriveInput) -> TokenStream {
    let name = &input.ident;
    let label = name.to_string();
    let (ty, put, take) = match &input.data {
        Data::Struct(s) => {
            let (tys, pat, make) = parts(&s.fields, quote!(#name));
            let vars = (0..tys.len()).map(|i| quote::format_ident!("f{i}"));
            (
                quote!(::ark5::ty::Ty::Tuple(vec![#(<#tys as ::ark5::ir::Node>::ty()),*])),
                quote!(let #pat = self; #(::ark5::ir::Node::put(#vars, out);)*),
                quote!(Ok(#make)),
            )
        }
        Data::Enum(e) => {
            let n = e.variants.len();
            let (mut tys, mut puts, mut takes) = (vec![], vec![], vec![]);
            for (i, v) in e.variants.iter().enumerate() {
                let (vname, i8) = (&v.ident, i as u8);
                let (ftys, pat, make) = parts(&v.fields, quote!(#name::#vname));
                let vars = (0..ftys.len()).map(|i| quote::format_ident!("f{i}"));
                tys.push(quote!(vec![#(<#ftys as ::ark5::ir::Node>::ty()),*]));
                puts.push(quote!(#pat => { out.push(#i8); #(::ark5::ir::Node::put(#vars, out);)* }));
                takes.push(quote!(#i8 => #make,));
            }
            (
                quote!(::ark5::ty::Ty::Sum(vec![#(#tys),*])),
                quote!(match self { #(#puts)* }),
                quote!(Ok(match r.variant(#n)? { #(#takes)* _ => unreachable!() })),
            )
        }
        Data::Union(_) => return quote!(compile_error!("a union is not a Node");),
    };
    quote! {
        impl ::ark5::ir::Node for #name {
            fn ty() -> ::ark5::ty::Ty {
                ::ark5::ir::named(#label, || #ty)
            }
            #[allow(unused_variables)]
            fn put(&self, out: &mut Vec<u8>) {
                #put
            }
            #[allow(unused_variables)]
            fn take(r: &mut ::ark5::codec::Reader) -> Result<Self, ::ark5::Refusal> {
                #take
            }
        }
    }
}
