extern crate proc_macro;

use std::iter::FromIterator;

#[proc_macro_attribute]
pub fn test(
    _args: proc_macro::TokenStream,
    mut ts: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let ts_clone = ts.clone();
    let func = syn::parse_macro_input!(ts_clone as syn::ItemFn);

    let ident = &func.sig.ident;
    let test_name = ident.to_string();
    let test_case_ident = quote::format_ident!("TEST_CASE_{}", ident);
    let result = quote::quote! {
        #[test_case]
        const #test_case_ident: ::mycelium_util::testing::TestCase = ::mycelium_util::testing::TestCase {
            name: #test_name,
            func: || ::mycelium_util::testing::TestResult::into_result(#ident()),
        };
    };

    // Go to some lengths to preserve the `TokenStream` instance, to avoid rustc
    // messing up the spans.
    ts.extend(proc_macro::TokenStream::from(result));
    ts
}

