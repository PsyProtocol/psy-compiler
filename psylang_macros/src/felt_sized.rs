use proc_macro2::TokenStream;
use quote::{quote, quote_spanned, ToTokens};
use syn::{parse_quote, spanned::Spanned, Data, DeriveInput, Fields, GenericParam, Generics, Index};

pub fn derive_felt_sized(input: proc_macro2::TokenStream) -> proc_macro2::TokenStream {
    // Parse the input tokens into a syntax tree.
    let input = syn::parse2(input).unwrap();

    // Delegate to the core implementation.
    derive_felt_sized_core(input)
}

pub fn derive_felt_sized_core(input: DeriveInput) -> proc_macro2::TokenStream {
    // Parse the input tokens into a syntax tree.

    // Used in the quasi-quotation below as `#name`.
    let name = input.ident;

    // Add a bound `T: FeltSized` to every type parameter T.
    let generics = add_trait_bounds(input.generics);
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    // Generate an expression to sum up the heap size of each field.
    let sum = felt_sized_sum(&input.data);

    let expanded = quote! {
        // The generated impl.
        impl #impl_generics psy_vm::dpn::ops::context_trait::FeltSized for #name #ty_generics #where_clause {
            fn size() -> u64 {
                #sum
            }
        }
    };

    // Hand the output tokens back to the compiler.
    proc_macro2::TokenStream::from(expanded)
}

// Add a bound `T: FeltSized` to every type parameter T.
fn add_trait_bounds(mut generics: Generics) -> Generics {
    for param in &mut generics.params {
        if let GenericParam::Type(ref mut type_param) = *param {
            type_param.bounds.push(parse_quote!(psy_vm::dpn::ops::context_trait::FeltSized));
        }
    }
    generics
}

// Generate an expression to sum up the heap size of each field.
fn felt_sized_sum(data: &Data) -> TokenStream {
    match *data {
        Data::Struct(ref data) => {
            match data.fields {
                Fields::Named(ref fields) => {
                    // Expands to an expression like
                    //
                    //     0 + self.x.felt_sized() + self.y.felt_sized() + self.z.felt_sized()
                    //
                    // but using fully qualified function call syntax.
                    //
                    // We take some care to use the span of each `syn::Field` as
                    // the span of the corresponding `felt_sized_of_children`
                    // call. This way if one of the field types does not
                    // implement `FeltSized` then the compiler's error message
                    // underlines which field it is. An example is shown in the
                    // readme of the parent directory.
                    let recurse = fields.named.iter().map(|f| {
                        //let name = &f.ident;
                        let ty = &f.ty;
                        quote_spanned! {f.span()=>
                            <#ty as psy_vm::dpn::ops::context_trait::FeltSized>::size()
                        }
                    });
                    quote! {
                        0 #(+ #recurse)*
                    }
                }
                Fields::Unnamed(ref fields) => {
                    // Expands to an expression like
                    //
                    //     0 + self.0.felt_sized() + self.1.felt_sized() + self.2.felt_sized()
                    let recurse = fields.unnamed.iter().enumerate().map(|(i, f)| {
                        let index = Index::from(i);
                        let ty = &f.ty;
                        quote_spanned! {f.span()=>
                            <#ty as psy_vm::dpn::ops::context_trait::FeltSized>::size()
                        }
                    });
                    quote! {
                        0 #(+ #recurse)*
                    }
                }
                Fields::Unit => {
                    // Unit structs cannot own more than 0 bytes of heap memory.
                    quote!(0)
                }
            }
        }
        Data::Enum(_) | Data::Union(_) => unimplemented!(),
    }
}
