use psy_abi::TypeRef;

#[test]
fn same_named_module_types_keep_their_circuit_widths() -> anyhow::Result<()> {
    let a = "pub mod a { pub struct Data { pub x: Felt, } pub struct SmallRecord { pub item: Data, } pub type Word = u32; }";
    let b = "pub mod b { pub struct Data { pub x: Felt, pub y: Felt, } pub struct LargeRecord { pub item: Data, } pub type Word = u64; }";
    for modules in [format!("{a} {b}"), format!("{b} {a}")] {
        let artifact = psy_interpreter::compile(&format!(r#"
            {modules}
            use a::SmallRecord;
            use b::LargeRecord;
            #[contract]
            pub struct Example {{ pub small: SmallRecord, pub large: LargeRecord, }}
            impl Example {{
                #[contract::view_method]
                pub fn small(value: a::Data) -> a::Data {{ value }}
                #[contract::view_method]
                pub fn large(value: b::Data) -> b::Data {{ value }}
                #[contract::view_method]
                pub fn narrow(value: a::Word) -> a::Word {{ value }}
                #[contract::view_method]
                pub fn wide(value: b::Word) -> b::Word {{ value }}
            }}
        "#))?;
        let abi = &artifact.abi;
        for (name, width) in [("small", 1), ("large", 2), ("narrow", 1), ("wide", 2)] {
            let method = abi.contract.methods.iter().find(|m| m.name == name).unwrap();
            let circuit = artifact.circuit_definitions.iter().find(|c| c.method_id == method.method_id).unwrap();
            assert_eq!((method.input_felt_count, method.output_felt_count), (width, width));
            assert_eq!((circuit.circuit_inputs.len(), circuit.circuit_outputs.len()), (width, width));
        }
        for (index, name, width) in [(0, "a::Data", 1), (1, "b::Data", 2)] {
            let state = &abi.contract.state[index];
            let TypeRef::Struct { name: record } = &state.ty else { panic!("missing record") };
            let field = &abi.types.iter().find(|ty| &ty.name == record).unwrap().fields[0];
            assert_eq!(field.ty, TypeRef::Struct { name: name.into() });
            assert_eq!(state.felt_size, width);
            let method = abi.contract.methods.iter().find(|m| m.name == state.name).unwrap();
            assert_eq!(method.inputs[0].ty, field.ty);
            assert_eq!(method.outputs[0].ty, field.ty);
            assert_eq!(abi.types.iter().find(|ty| ty.name == name).unwrap().felt_size, width);
        }
    }
    Ok(())
}

#[test]
fn unique_qualified_type_keeps_its_short_abi_name() -> anyhow::Result<()> {
    let artifact = psy_interpreter::compile(r#"
        pub mod values { pub struct Only { pub x: Felt, } pub type Word = u64; }
        #[contract]
        pub struct Example { pub x: Felt, }
        impl Example {
            #[contract::view_method]
            pub fn echo(value: values::Only) -> values::Only { value }
            #[contract::view_method]
            pub fn word(value: values::Word) -> values::Word { value }
        }
    "#)?;
    let echo = artifact.abi.contract.methods.iter().find(|m| m.name == "echo").unwrap();
    assert_eq!(echo.inputs[0].ty, TypeRef::Struct { name: "Only".into() });
    assert_eq!((echo.input_felt_count, echo.output_felt_count), (1, 1));
    let word = artifact.abi.contract.methods.iter().find(|m| m.name == "word").unwrap();
    assert_eq!((word.input_felt_count, word.output_felt_count), (2, 2));
    Ok(())
}

#[test]
fn standard_map_keeps_its_storage_layout_with_a_user_map() -> anyhow::Result<()> {
    let artifact = psy_interpreter::compile(r#"
        pub mod custom { pub struct Map { pub number: Felt, } }
        #[contract]
        #[derive(Storage)]
        pub struct Example { pub prefix: Felt, pub items: Map<Hash, Hash, 32u32>, pub tail: Felt, }
        impl Example {
            #[contract::view_method]
            pub fn echo(value: custom::Map) -> custom::Map { value }
        }
    "#)?;
    let abi = artifact.abi.contract;
    assert!(matches!(abi.state[1].ty, TypeRef::Map { capacity: 32, .. }));
    assert_eq!((abi.state[1].offset, abi.state[1].felt_size, abi.state[2].offset), (4, 132, 136));
    assert_eq!(abi.methods[0].inputs[0].ty, TypeRef::Struct { name: "custom::Map".into() });
    Ok(())
}

#[test]
fn qualified_generic_struct_uses_the_instantiated_layout() -> anyhow::Result<()> {
    let artifact = psy_interpreter::compile(r#"
        pub mod a { pub struct Envelope<T> { pub value: T, } pub struct Flag<B: bool> { pub value: Felt, } }
        #[contract]
        pub struct Example { pub x: Felt, }
        impl Example {
            #[contract::view_method]
            pub fn echo(value: a::Envelope<Felt>) -> a::Envelope<Felt> { value }
            #[contract::view_method]
            pub fn wide(value: a::Envelope<u64>) -> a::Envelope<u64> { value }
            #[contract::view_method]
            pub fn enabled(value: a::Flag<true>) -> a::Flag<true> { value }
            #[contract::view_method]
            pub fn disabled(value: a::Flag<false>) -> a::Flag<false> { value }
        }
    "#)?;
    for (method_name, type_name, width) in [
        ("echo", "Envelope<Felt>", 1), ("wide", "Envelope<u64>", 2),
        ("enabled", "Flag<true>", 1), ("disabled", "Flag<false>", 1),
    ] {
        let method = artifact.abi.contract.methods.iter().find(|m| m.name == method_name).unwrap();
        assert_eq!((method.input_felt_count, method.output_felt_count), (width, width));
        assert_eq!(method.inputs[0].ty, TypeRef::Struct { name: type_name.into() });
        let layout = artifact.abi.types.iter().find(|entry| entry.name == type_name).unwrap();
        assert_eq!((layout.felt_size, layout.fields[0].felt_size), (width, width));
    }
    Ok(())
}
