use std::mem;
pub trait HeapSize {
    /// Total number of bytes of heap memory owned by `self`.
    ///
    /// Does not include the size of `self` itself, which may or may not be on
    /// the heap. Includes only children of `self`, meaning things pointed to by
    /// `self`.
    fn heap_size_of_children(&self) -> usize;
}

//
// In a real version of this library there would be lots more impls here, but
// here are some interesting ones.
//

impl HeapSize for u8 {
    /// A `u8` does not own any heap memory.
    fn heap_size_of_children(&self) -> usize {
        0
    }
}

impl HeapSize for String {
    /// A `String` owns enough heap memory to hold its reserved capacity.
    fn heap_size_of_children(&self) -> usize {
        self.capacity()
    }
}

impl<T> HeapSize for Box<T>
where
    T: ?Sized + HeapSize,
{
    /// A `Box` owns however much heap memory was allocated to hold the value of
    /// type `T` that we placed on the heap, plus transitively however much `T`
    /// itself owns.
    fn heap_size_of_children(&self) -> usize {
        mem::size_of_val(&**self) + (**self).heap_size_of_children()
    }
}

impl<T> HeapSize for [T]
where
    T: HeapSize,
{
    /// Sum of heap memory owned by each element of a dynamically sized slice of
    /// `T`.
    fn heap_size_of_children(&self) -> usize {
        self.iter().map(HeapSize::heap_size_of_children).sum()
    }
}

impl<'a, T> HeapSize for &'a T
where
    T: ?Sized,
{
    /// A shared reference does not own heap memory.
    fn heap_size_of_children(&self) -> usize {
        0
    }
}

use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::{parse_macro_input, parse_quote, spanned::Spanned, Data, DeriveInput, Fields, GenericParam, Generics, Index};

// Add a bound `T: HeapSize` to every type parameter T.
pub fn add_trait_bounds(mut generics: Generics) -> Generics {
    for param in &mut generics.params {
        if let GenericParam::Type(ref mut type_param) = *param {
            type_param.bounds.push(parse_quote!(heapsize::HeapSize));
        }
    }
    generics
}

// Generate an expression to sum up the heap size of each field.
pub fn heap_size_sum(data: &Data) -> TokenStream {
    match *data {
        Data::Struct(ref data) => {
            match data.fields {
                Fields::Named(ref fields) => {
                    // Expands to an expression like
                    //
                    //     0 + self.x.heap_size() + self.y.heap_size() + self.z.heap_size()
                    //
                    // but using fully qualified function call syntax.
                    //
                    // We take some care to use the span of each `syn::Field` as
                    // the span of the corresponding `heap_size_of_children`
                    // call. This way if one of the field types does not
                    // implement `HeapSize` then the compiler's error message
                    // underlines which field it is. An example is shown in the
                    // readme of the parent directory.
                    let recurse = fields.named.iter().map(|f| {
                        let name = &f.ident;
                        quote_spanned! {f.span()=>
                            heapsize::HeapSize::heap_size_of_children(&self.#name)
                        }
                    });
                    quote! {
                        0 #(+ #recurse)*
                    }
                }
                Fields::Unnamed(ref fields) => {
                    // Expands to an expression like
                    //
                    //     0 + self.0.heap_size() + self.1.heap_size() + self.2.heap_size()
                    let recurse = fields.unnamed.iter().enumerate().map(|(i, f)| {
                        let index = Index::from(i);
                        quote_spanned! {f.span()=>
                            heapsize::HeapSize::heap_size_of_children(&self.#index)
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
