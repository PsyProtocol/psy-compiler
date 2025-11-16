use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::{parse_macro_input, parse_quote, spanned::Spanned, Data, DeriveInput, Fields, GenericParam, Generics, Ident, Index};

pub fn derive_state_init(input: proc_macro2::TokenStream) -> proc_macro2::TokenStream {
    // Parse the input tokens into a syntax tree.
    let input = syn::parse2(input).unwrap();

    // Delegate to the core implementation.
    derive_state_init_core(input)
}

pub fn derive_state_init_core(input: DeriveInput) -> proc_macro2::TokenStream {
    // Parse the input tokens into a syntax tree.

    // Used in the quasi-quotation below as `#name`.
    let name = input.ident;

    // Add a bound `T: FeltSized` to every type parameter T.
    let generics = add_trait_bounds(input.generics);
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    // Generate an expression to sum up the heap size of each field.
    let sum = state_init_sum(&input.data);

    let expanded = quote! {
        // The generated impl.
        impl #impl_generics psy_vm::dpn::ops::sym_felt::QStateInitializable for #name #ty_generics #where_clause {
            fn create_stateful_at<CTXT: DPNContext<SymFeltRef>>(context: &mut CTXT, state_pointer: SymFeltRef, contract_state_tree_height: u16, contract_id: SymFeltRef, user_id: SymFeltRef) -> Self {
                let mut cur_offset = 0u64;
                let mut nw_pointer = SymFeltRef::new_constant(cur_offset);
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
            type_param.bounds.push(parse_quote!(psy_vm::dpn::ops::sym_felt::QStateInitializable));
        }
    }
    generics
}

// Generate an expression to sum up the heap size of each field.
fn state_init_sum(data: &Data) -> TokenStream {
    match *data {
        Data::Struct(ref data) => {
            match data.fields {
                Fields::Named(ref fields) => {
                    // Expands to an expression like
                    //
                    //     0 + self.x.state_init() + self.y.state_init() + self.z.state_init()
                    //
                    // but using fully qualified function call syntax.
                    //
                    // We take some care to use the span of each `syn::Field` as
                    // the span of the corresponding `state_init_of_children`
                    // call. This way if one of the field types does not
                    // implement `FeltSized` then the compiler's error message
                    // underlines which field it is. An example is shown in the
                    // readme of the parent directory.
                    let (defs, create_fields): (Vec<TokenStream>, Vec<TokenStream>) = fields.named.iter().enumerate().map(|(i, f)| {
                        let name = &f.ident;
                        let ty = &f.ty;
                        let ident1 = Ident::new_raw(&format!("f_t_{}", i), f.span());
                        (quote_spanned! {f.span()=>
                            nw_pointer = context.op_add(state_pointer, SymFeltRef::new_constant(cur_offset));
                            let #ident1 = <#ty as QStateInitializable>::create_stateful_at(context, nw_pointer, contract_state_tree_height, contract_id, user_id);
                            cur_offset = cur_offset + <#ty as FeltSized>::size();
                        }, quote_spanned! {f.span()=>
                            #name: #ident1,
                        })
                    }).unzip();
                    quote! {
                        #(#defs)*
                        Self {
                            #(#create_fields)*
                        }
                    }
                }
                Fields::Unnamed(ref fields) => {
                    // Expands to an expression like
                    //
                    //     0 + self.x.state_init() + self.y.state_init() + self.z.state_init()
                    //
                    // but using fully qualified function call syntax.
                    //
                    // We take some care to use the span of each `syn::Field` as
                    // the span of the corresponding `state_init_of_children`
                    // call. This way if one of the field types does not
                    // implement `FeltSized` then the compiler's error message
                    // underlines which field it is. An example is shown in the
                    // readme of the parent directory.
                    let (defs, create_fields): (Vec<TokenStream>, Vec<TokenStream>) = fields.unnamed.iter().enumerate().map(|(i, f)| {
                        let ty = &f.ty;
                        let ident1 = Ident::new_raw(&format!("f_t_{}", i), f.span());
                        (quote_spanned! {f.span()=>
                            nw_pointer = context.op_add(state_pointer, SymFeltRef::new_constant(cur_offset));
                            let #ident1 = <#ty as QStateInitializable>::create_stateful_at(context, nw_pointer, contract_state_tree_height, contract_id, user_id);
                            cur_offset = cur_offset + <#ty as FeltSized>::size();
                        }, quote_spanned! {f.span()=>
                            #ident1,
                        })
                    }).unzip();
                    quote! {
                        #(#defs)*
                        Self(
                            #(#create_fields)*
                        )
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
