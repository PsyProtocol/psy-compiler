// Direct unit coverage for the small psy-ast conversion/accessor impls that
// source-driven tests never reach: id conversions, NodeInfo helpers, Display
// impls, Program's Index/IndexMut wiring, and module-graph printing.

use psy_common::FileId;

use crate::{
    Comment, CommentNode, CrateId, DefId, ExprId, GenericQualifier, Ident, IdentId, Identifier, Location, MatchPattern, ModuleId, ModuleNode,
    NodeInfo, NodeType, Program, StmtNode, UncheckedType, Visibility,
};

#[test]
fn id_and_node_conversions_round_trip() {
    let module_id = ModuleId(3);
    assert_eq!(CrateId::from_module_id(module_id), CrateId(3));
    assert_eq!(CrateId::from(module_id), CrateId(3));
    assert_eq!(CrateId::from(&module_id), CrateId(3));
    assert_eq!(ModuleId::from(CrateId(3)), module_id);

    assert!(GenericQualifier::new(true).is_const);
    assert!(!GenericQualifier::new(false).is_const);
}

#[test]
fn node_info_helpers_and_displays_cover_every_shape() {
    // Statement accessors and From impls.
    let expression = StmtNode::Expression(ExprId(5));
    assert_eq!(expression.as_expression(), Some(&ExprId(5)));
    assert_eq!(expression.as_definition(), None);
    let definition = StmtNode::Definition(DefId(6));
    assert_eq!(definition.as_definition(), Some(&DefId(6)));
    assert_eq!(definition.as_expression(), None);
    assert!(matches!(StmtNode::from(ExprId(7)), StmtNode::Expression(ExprId(7))));
    assert!(matches!(StmtNode::from(DefId(8)), StmtNode::Definition(DefId(8))));

    // Match patterns expose their locations.
    let location = Location::new(FileId(1), 2, 3);
    assert_eq!(MatchPattern::Value(ExprId(0), location).location(), location);
    assert_eq!(MatchPattern::PlaceHolder(location).location(), location);

    // Comments expose location and render with their delimiters.
    let line = Comment::new_line("note".to_string(), location);
    let block = Comment::new_block("aside".to_string(), location);
    assert_eq!(line.location(), location);
    assert_eq!(block.location(), location);
    assert_eq!(line.to_string(), "// note");
    assert_eq!(block.to_string(), "/* aside */");

    // Comment definitions report their node type.
    assert_eq!(CommentNode { comments: vec![line], location }.node_type(), NodeType::Comment);

    // basic_target only resolves for basic types.
    let basic = UncheckedType::Basic(Identifier::new(IdentId(1), location));
    assert_eq!(basic.basic_target(), Some(Identifier::new(IdentId(1), location)));
    assert_eq!(UncheckedType::Unknown.basic_target(), None);
}

#[test]
fn identifier_equality_compares_against_bare_ids() {
    let location = Location::default();
    let identifier = Identifier::new(IdentId(4), location);
    assert!(IdentId(4) == identifier);
    assert!(identifier == IdentId(4));
    assert!(IdentId(5) != identifier);
    assert!(identifier != IdentId(5));

    let mut interner = crate::Interner::new();
    let id = interner.intern_ident("plain");
    interner[id] = Ident::from("renamed");
    assert_eq!(interner[id], Ident::from("renamed"));
}

#[test]
fn program_indexes_mutate_and_print_the_module_graph() {
    let mut program = Program::<u32>::new();
    let location = Location::default();

    let alpha_name = program.interner.intern_ident("alpha");
    let alpha = program.modules.add_node(ModuleNode::new(
        Identifier::new(alpha_name, location),
        FileId(0),
        Visibility::Public,
        Vec::new(),
        &mut program.defs,
        Vec::new(),
        location,
    ));
    let beta_name = program.interner.intern_ident("beta");
    let beta = program.modules.add_node(ModuleNode::new(
        Identifier::new(beta_name, location),
        FileId(0),
        Visibility::Public,
        Vec::new(),
        &mut program.defs,
        Vec::new(),
        location,
    ));

    program.add_module_child(Some(alpha), beta);
    program.dependency_graph.add_node(CrateId::from_module_id(alpha));
    program.dependency_graph.add_node(CrateId::from_module_id(beta));
    program.dependency_graph.add_edge(CrateId::from_module_id(beta), CrateId::from_module_id(alpha));
    assert_eq!(program.modules.iter().count(), 2);

    // IndexMut through Program for every arena.
    let stmt_id = program.stmts.alloc_item(StmtNode::Expression(ExprId(0)));
    program[stmt_id] = StmtNode::Expression(ExprId(1));
    assert_eq!(program[stmt_id].as_expression(), Some(&ExprId(1)));

    program[alpha].data_mut().visibility = Visibility::Private;
    assert_eq!(program.modules[alpha].data().visibility, Visibility::Private);

    program[alpha_name] = Ident::from("renamed_alpha");
    assert_eq!(program.module_name(alpha), &Ident::from("renamed_alpha"));

    // Printing must walk modules, children, and dependencies without panicking.
    program.print_module_graph();
}
