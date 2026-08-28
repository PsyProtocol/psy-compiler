//! Item parsing for the recursive-descent parser.
//! Handles modules and all definition forms accepted by the LALRPOP grammar.

use indexmap::IndexMap;
use psy_ast::{
    AssociatedType, AssociatedTypeValue, AttrNode, Comment, ConstNode, DefinitionNode, EnumNode, EnumVariant, FunctionNode, FunctionParameter,
    IdentId, Identifier, ImplNode, ModuleNode, Qualifier, StructField, StructNode, TraitImplNode, TraitNode, TypeAliasNode, TypeQualifier,
    UncheckedType, UseNode, Visibility,
};
use psy_lexer::Token;
use psy_vm::dpn::ops::context_trait::{ContextFelt, DPNContext};

use crate::{
    error::{Error, ExpectedToken, Result},
    recursive::ParsedModuleItem,
};

/// Parse one module item, including the comments immediately preceding it.
pub fn parse_module_item<'src, 'p, F, C>(parser: &mut super::ModuleParser<'src, 'p, F, C>, comments: Vec<Comment>) -> Result<ParsedModuleItem>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    let start = comments
        .first()
        .map(Comment::location)
        .map(|location| location.start)
        .unwrap_or_else(|| parser.cursor.peek_start());
    let attrs = parse_attributes(parser)?;
    let visibility = parse_visibility(parser);

    if parser.cursor.at(&Token::KeywordMod) {
        if !attrs.is_empty() {
            return Err(unexpected_item(parser));
        }
        return parse_module(parser, comments, visibility, start);
    }

    let definition = match parser.cursor.peek() {
        Some(Token::KeywordUse) if attrs.is_empty() => parse_use_definition(parser, comments, visibility, start)?,
        Some(Token::KeywordConst) if attrs.is_empty() && !matches!(parser.cursor.lookahead(1), Some(Token::KeywordExtern | Token::KeywordFn)) => {
            parse_const_definition(parser, comments, visibility, start)?
        }
        Some(Token::KeywordFn | Token::KeywordConst | Token::KeywordExtern) => {
            let function = parse_function_definition(parser, comments, attrs, visibility, start, FunctionContext::TopLevel)?;
            validate_top_level_function(&function)?;
            function
        }
        Some(Token::KeywordStruct) => parse_struct_definition(parser, comments, attrs, visibility, start)?,
        Some(Token::KeywordEnum) if attrs.is_empty() => parse_enum_definition(parser, comments, visibility, start)?,
        Some(Token::KeywordImpl) => {
            if visibility == Visibility::Public {
                return Err(unexpected_item(parser));
            }
            parse_impl_definition(parser, comments, attrs, start)?
        }
        Some(Token::KeywordTrait) if attrs.is_empty() => parse_trait_definition(parser, comments, visibility, start)?,
        Some(Token::KeywordType) if attrs.is_empty() => parse_type_alias_definition(parser, comments, visibility, start)?,
        _ => return Err(unexpected_item(parser)),
    };

    Ok(ParsedModuleItem::Definition(definition))
}

fn parse_module<'src, 'p, F, C>(
    parser: &mut super::ModuleParser<'src, 'p, F, C>,
    mut comments: Vec<Comment>,
    visibility: Visibility,
    start: usize,
) -> Result<ParsedModuleItem>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    parser.cursor.expect(&Token::KeywordMod)?;
    let name = parser.parse_identifier()?;

    if let Some((_, end)) = parser.cursor.eat(&Token::Semicolon) {
        if !comments.is_empty() {
            return Err(Error::UnexpectedToken {
                found: "comment before external module declaration".into(),
                expected: vec![ExpectedToken::Keyword("module declaration without comments")],
                location: comments[0].location(),
            });
        }
        return Ok(ParsedModuleItem::ExternalModule(name, visibility, parser.location(start, end)));
    }

    parser.cursor.expect(&Token::LBrace)?;
    let mut modules = Vec::new();
    let mut inline_modules = Vec::new();
    let mut definitions = Vec::new();

    loop {
        let item_comments = parser.cursor.take_leading_comments();
        if parser.cursor.at(&Token::RBrace) {
            comments.extend(item_comments);
            break;
        }
        if parser.cursor.at_end() {
            return Err(Error::UnexpectedEof {
                expected: vec![ExpectedToken::Symbol("}".into())],
                location: parser.cursor.eof_location(),
            });
        }

        match parse_module_item(parser, item_comments)? {
            ParsedModuleItem::ExternalModule(name, visibility, location) => {
                modules.push((name, visibility, location));
            }
            ParsedModuleItem::InlineModule(module) => inline_modules.push(module),
            ParsedModuleItem::Definition(definition) => {
                definitions.push(parser.alloc_def(definition));
            }
        }
    }

    let (_, end) = parser.cursor.expect(&Token::RBrace)?;
    Ok(ParsedModuleItem::InlineModule(ModuleNode {
        name,
        file_id: parser.cursor.file_id(),
        modules,
        inline_modules,
        definitions,
        visibility,
        comments,
        location: parser.location(start, end),
    }))
}

fn parse_use_definition<'src, 'p, F, C>(
    parser: &mut super::ModuleParser<'src, 'p, F, C>,
    comments: Vec<Comment>,
    visibility: Visibility,
    start: usize,
) -> Result<DefinitionNode>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    parser.cursor.expect(&Token::KeywordUse)?;
    let kind = parse_use_root(parser)?;
    parser.cursor.expect(&Token::DoubleColon)?;

    let mut path = Vec::new();
    let target = loop {
        if parser.cursor.at(&Token::OperatorMul) {
            parser.cursor.advance();
            break None;
        }

        let identifier = parser.parse_identifier()?;
        if parser.cursor.eat(&Token::DoubleColon).is_some() {
            path.push(identifier);
        } else {
            break Some(identifier);
        }
    };

    let (_, end) = parser.cursor.expect(&Token::Semicolon)?;
    Ok(DefinitionNode::Use(UseNode {
        visibility,
        kind,
        segments: path,
        target,
        comments,
        location: parser.location(start, end),
    }))
}

fn parse_use_root<'src, 'p, F, C>(parser: &mut super::ModuleParser<'src, 'p, F, C>) -> Result<Identifier>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    let start = parser.cursor.peek_start();
    let end = parser.cursor.peek_end();
    let location = parser.location(start, end);
    match parser.cursor.peek() {
        Some(Token::KeywordSelf) => {
            parser.cursor.advance();
            Ok(Identifier::new(IdentId::SELF, location))
        }
        Some(Token::KeywordSuper) => {
            parser.cursor.advance();
            Ok(Identifier::new(IdentId::SUPER, location))
        }
        Some(Token::KeywordCrate) => {
            parser.cursor.advance();
            Ok(Identifier::new(IdentId::CRATE, location))
        }
        _ => parser.parse_identifier(),
    }
}

fn parse_const_definition<'src, 'p, F, C>(
    parser: &mut super::ModuleParser<'src, 'p, F, C>,
    comments: Vec<Comment>,
    visibility: Visibility,
    start: usize,
) -> Result<DefinitionNode>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    parser.cursor.expect(&Token::KeywordConst)?;
    let name = parser.parse_identifier()?;
    parser.cursor.expect(&Token::Colon)?;
    let ty = parse_const_declaration_type(parser)?;
    parser.cursor.expect(&Token::Assign)?;
    let value = parser.parse_expression_struct_allowed()?;
    let value = parser.alloc_expr(value);
    let (_, end) = parser.cursor.expect(&Token::Semicolon)?;

    Ok(DefinitionNode::Const(ConstNode {
        name,
        ty,
        value,
        visibility,
        comments,
        location: parser.location(start, end),
    }))
}
fn parse_const_declaration_type<'src, 'p, F, C>(parser: &mut super::ModuleParser<'src, 'p, F, C>) -> Result<UncheckedType>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    let (id, start, end) = match parser.cursor.peek() {
        Some(Token::TypeFelt) => (IdentId::TYPE_FELT, parser.cursor.peek_start(), parser.cursor.peek_end()),
        Some(Token::TypeBool) => (IdentId::TYPE_BOOL, parser.cursor.peek_start(), parser.cursor.peek_end()),
        Some(Token::TypeU32) => (IdentId::TYPE_U32, parser.cursor.peek_start(), parser.cursor.peek_end()),
        _ => return Err(parser.unexpected(vec![ExpectedToken::Type])),
    };
    parser.cursor.advance();
    Ok(UncheckedType::Basic(Identifier::new(id, parser.location(start, end))))
}

#[derive(Clone, Copy)]
enum FunctionContext {
    TopLevel,
    Impl,
    Trait,
}

fn parse_function_definition<'src, 'p, F, C>(
    parser: &mut super::ModuleParser<'src, 'p, F, C>,
    comments: Vec<Comment>,
    attrs: Vec<AttrNode>,
    visibility: Visibility,
    start: usize,
    context: FunctionContext,
) -> Result<DefinitionNode>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    let qualifier_start = parser.cursor.peek_start();
    let mut is_const = false;
    let mut is_extern = false;

    loop {
        match parser.cursor.peek() {
            Some(Token::KeywordConst) if !is_const => {
                parser.cursor.advance();
                is_const = true;
            }
            Some(Token::KeywordExtern) if !is_extern => {
                parser.cursor.advance();
                is_extern = true;
            }
            _ => break,
        }
    }
    let qualifier_end = parser.cursor.peek_start();
    parser.cursor.expect(&Token::KeywordFn)?;

    let name = parser.parse_identifier()?;
    let generic_parameters = parser.parse_generic_parameters()?;
    parser.cursor.expect(&Token::LParen)?;
    let parameters = parse_function_parameters(parser)?;
    parser.cursor.expect(&Token::RParen)?;
    let return_type = parser.parse_function_return()?;

    let (body, end) = if parser.cursor.at(&Token::LBrace) {
        let body = parser.parse_block_expression()?;
        let end = parser.expression_location(&body).end;
        (Some(parser.alloc_expr(body)), end)
    } else {
        let (_, end) = parser.cursor.expect(&Token::Semicolon)?;
        (None, end)
    };

    let body_required = match context {
        FunctionContext::TopLevel => !is_extern,
        FunctionContext::Impl => true,
        FunctionContext::Trait => false,
    };
    if body.is_none() && body_required {
        return Err(Error::FunctionBodyMissing);
    }
    validate_self_parameter_position(&parameters)?;

    Ok(DefinitionNode::Function(FunctionNode {
        name,
        parameters,
        generic_parameters,
        body,
        return_type,
        qualifier: Qualifier {
            is_extern,
            is_const,
            location: parser.location(qualifier_start, qualifier_end),
        },
        visibility,
        attrs,
        comments,
        location: parser.location(start, end),
    }))
}

fn parse_function_parameters<'src, 'p, F, C>(parser: &mut super::ModuleParser<'src, 'p, F, C>) -> Result<Vec<FunctionParameter>>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    let mut parameters = Vec::new();
    if parser.cursor.at(&Token::RParen) {
        return Ok(parameters);
    }

    loop {
        parameters.push(parse_function_parameter(parser)?);
        if parser.cursor.eat(&Token::Comma).is_none() || parser.cursor.at(&Token::RParen) {
            break;
        }
    }
    Ok(parameters)
}

fn parse_function_parameter<'src, 'p, F, C>(parser: &mut super::ModuleParser<'src, 'p, F, C>) -> Result<FunctionParameter>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    let start = parser.cursor.peek_start();
    let qualifier_start = start;
    let is_mutable = parser.cursor.eat(&Token::KeywordMut).is_some();
    let qualifier_end = parser.cursor.peek_start();
    let name = parser.parse_identifier_or_self()?;

    let ty = if parser.cursor.eat(&Token::Colon).is_some() {
        parser.parse_path_ty()?
    } else if name.id == IdentId::SELF {
        UncheckedType::Basic(Identifier::new(IdentId::TYPE_SELF, name.location))
    } else {
        return Err(parser.unexpected(vec![ExpectedToken::Symbol(":".into())]));
    };
    let end = ty.location().end;

    Ok(FunctionParameter::new(
        name,
        TypeQualifier {
            is_mutable,
            location: parser.location(qualifier_start, qualifier_end),
        },
        ty,
        parser.location(start, end),
    ))
}

fn validate_top_level_function(definition: &DefinitionNode) -> Result<()> {
    let function = definition.as_function().expect("function parser always returns a function definition");
    if function
        .parameters
        .iter()
        .any(|parameter| parameter.name.id == IdentId::SELF || matches!(&parameter.ty, UncheckedType::Basic(ty) if ty.id == IdentId::TYPE_SELF))
    {
        Err(Error::InvalidSelfParameter)
    } else {
        Ok(())
    }
}

fn validate_self_parameter_position(parameters: &[FunctionParameter]) -> Result<()> {
    if parameters
        .iter()
        .enumerate()
        .any(|(index, parameter)| parameter.name.id == IdentId::SELF && index != 0)
    {
        Err(Error::InvalidSelfParameter)
    } else {
        Ok(())
    }
}

fn parse_struct_definition<'src, 'p, F, C>(
    parser: &mut super::ModuleParser<'src, 'p, F, C>,
    comments: Vec<Comment>,
    attrs: Vec<AttrNode>,
    visibility: Visibility,
    start: usize,
) -> Result<DefinitionNode>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    parser.cursor.expect(&Token::KeywordStruct)?;
    let name = parser.parse_identifier()?;
    let generic_parameters = parser.parse_generic_parameters()?;
    parser.cursor.expect(&Token::LBrace)?;
    let fields = parse_struct_fields(parser)?;
    let (_, end) = parser.cursor.expect(&Token::RBrace)?;

    Ok(DefinitionNode::Struct(StructNode {
        name,
        generic_parameters,
        fields,
        attrs,
        visibility,
        comments,
        location: parser.location(start, end),
        is_generated: false,
    }))
}

fn parse_struct_fields<'src, 'p, F, C>(parser: &mut super::ModuleParser<'src, 'p, F, C>) -> Result<IndexMap<Identifier, StructField>>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    let mut fields = IndexMap::new();
    if parser.cursor.at(&Token::RBrace) {
        return Ok(fields);
    }

    loop {
        let comments = parser.cursor.take_leading_comments();
        if parser.cursor.at(&Token::RBrace) {
            if !comments.is_empty() {
                return Err(Error::UnexpectedToken {
                    found: "comment before struct closing brace".into(),
                    expected: vec![ExpectedToken::Ident],
                    location: comments[0].location(),
                });
            }
            break;
        }
        let (name, field) = parse_struct_field(parser, comments)?;
        fields.insert(name, field);
        if parser.cursor.eat(&Token::Comma).is_none() || parser.cursor.at(&Token::RBrace) {
            break;
        }
    }
    Ok(fields)
}

fn parse_struct_field<'src, 'p, F, C>(parser: &mut super::ModuleParser<'src, 'p, F, C>, comments: Vec<Comment>) -> Result<(Identifier, StructField)>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    let start = comments
        .first()
        .map(Comment::location)
        .map(|location| location.start)
        .unwrap_or_else(|| parser.cursor.peek_start());
    let attrs = parse_attributes(parser)?;
    let visibility = parse_visibility(parser);
    let name = parser.parse_identifier()?;
    parser.cursor.expect(&Token::Colon)?;
    let ty = parser.parse_path_ty()?;
    let end = ty.location().end;

    Ok((
        name,
        StructField {
            ty,
            attrs,
            visibility,
            comments,
            location: parser.location(start, end),
        },
    ))
}

fn parse_enum_definition<'src, 'p, F, C>(
    parser: &mut super::ModuleParser<'src, 'p, F, C>,
    comments: Vec<Comment>,
    visibility: Visibility,
    start: usize,
) -> Result<DefinitionNode>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    parser.cursor.expect(&Token::KeywordEnum)?;
    let name = parser.parse_identifier()?;
    let generic_parameters = parser.parse_generic_parameters()?;
    parser.cursor.expect(&Token::LBrace)?;
    let variants = parse_enum_variants(parser)?;
    let (_, end) = parser.cursor.expect(&Token::RBrace)?;

    Ok(DefinitionNode::Enum(EnumNode {
        name,
        generic_parameters,
        variants,
        visibility,
        comments,
        location: parser.location(start, end),
    }))
}

fn parse_enum_variants<'src, 'p, F, C>(parser: &mut super::ModuleParser<'src, 'p, F, C>) -> Result<Vec<EnumVariant>>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    let mut variants = Vec::new();
    if parser.cursor.at(&Token::RBrace) {
        return Ok(variants);
    }

    loop {
        variants.push(parse_enum_variant(parser)?);
        if parser.cursor.eat(&Token::Comma).is_none() || parser.cursor.at(&Token::RBrace) {
            break;
        }
    }
    Ok(variants)
}

fn parse_enum_variant<'src, 'p, F, C>(parser: &mut super::ModuleParser<'src, 'p, F, C>) -> Result<EnumVariant>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    let name = parser.parse_identifier()?;
    if parser.cursor.eat(&Token::LParen).is_some() {
        let mut members = Vec::new();
        if !parser.cursor.at(&Token::RParen) {
            loop {
                members.push(parser.parse_path_ty()?);
                if parser.cursor.eat(&Token::Comma).is_none() || parser.cursor.at(&Token::RParen) {
                    break;
                }
            }
        }
        parser.cursor.expect(&Token::RParen)?;
        return Ok(EnumVariant::Tuple(name, members));
    }

    if parser.cursor.eat(&Token::LBrace).is_some() {
        let fields = parse_struct_fields(parser)?;
        parser.cursor.expect(&Token::RBrace)?;
        return Ok(EnumVariant::Struct(name, fields));
    }

    Ok(EnumVariant::Basic(name))
}

fn parse_impl_definition<'src, 'p, F, C>(
    parser: &mut super::ModuleParser<'src, 'p, F, C>,
    comments: Vec<Comment>,
    attrs: Vec<AttrNode>,
    start: usize,
) -> Result<DefinitionNode>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    parser.cursor.expect(&Token::KeywordImpl)?;
    let generic_parameters = parser.parse_generic_parameters()?;
    let first_ty = parser.parse_path_ty()?;
    let (trait_ty, ty) = if parser.cursor.eat(&Token::KeywordFor).is_some() {
        (Some(first_ty), parser.parse_path_ty()?)
    } else {
        (None, first_ty)
    };

    parser.cursor.expect(&Token::LBrace)?;
    let (associated_types, body, mut comments) = parse_impl_body(parser, comments)?;
    let (_, end) = parser.cursor.expect(&Token::RBrace)?;
    let location = parser.location(start, end);

    if let Some(trait_ty) = trait_ty {
        Ok(DefinitionNode::TraitImpl(TraitImplNode {
            generic_parameters,
            associated_types,
            trait_ty,
            ty,
            body,
            attrs,
            comments: std::mem::take(&mut comments),
            location,
            is_generated: false,
        }))
    } else {
        Ok(DefinitionNode::Impl(ImplNode {
            generic_parameters,
            associated_types,
            ty,
            body,
            attrs,
            comments: std::mem::take(&mut comments),
            location,
            is_generated: false,
        }))
    }
}

fn parse_impl_body<'src, 'p, F, C>(
    parser: &mut super::ModuleParser<'src, 'p, F, C>,
    mut owner_comments: Vec<Comment>,
) -> Result<(IndexMap<Identifier, AssociatedTypeValue>, Vec<psy_ast::DefId>, Vec<Comment>)>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    let mut associated_types = IndexMap::new();
    let mut body = Vec::new();
    let mut parsing_methods = false;

    loop {
        let comments = parser.cursor.take_leading_comments();
        if parser.cursor.at(&Token::RBrace) {
            owner_comments.extend(comments);
            break;
        }
        if parser.cursor.at_end() {
            return Err(Error::UnexpectedEof {
                expected: vec![ExpectedToken::Symbol("}".into())],
                location: parser.cursor.eof_location(),
            });
        }

        if starts_associated_type_value(parser) {
            if parsing_methods {
                return Err(unexpected_item(parser));
            }
            let (name, value) = parse_associated_type_value(parser, comments)?;
            associated_types.insert(name, value);
            continue;
        }

        parsing_methods = true;

        let attrs_start = comments
            .first()
            .map(Comment::location)
            .map(|location| location.start)
            .unwrap_or_else(|| parser.cursor.peek_start());
        let attrs = parse_attributes(parser)?;
        let visibility = parse_visibility(parser);
        let function = parse_function_definition(parser, comments, attrs, visibility, attrs_start, FunctionContext::Impl)?;
        body.push(parser.alloc_def(function));
    }

    Ok((associated_types, body, owner_comments))
}

fn starts_associated_type_value<'src, 'p, F, C>(parser: &super::ModuleParser<'src, 'p, F, C>) -> bool
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    matches!(
        (parser.cursor.lookahead(0), parser.cursor.lookahead(1)),
        (Some(Token::KeywordType), _) | (Some(Token::KeywordPub), Some(Token::KeywordType))
    )
}

fn parse_associated_type_value<'src, 'p, F, C>(
    parser: &mut super::ModuleParser<'src, 'p, F, C>,
    comments: Vec<Comment>,
) -> Result<(Identifier, AssociatedTypeValue)>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    let start = comments
        .first()
        .map(Comment::location)
        .map(|location| location.start)
        .unwrap_or_else(|| parser.cursor.peek_start());
    let visibility = parse_visibility(parser);
    parser.cursor.expect(&Token::KeywordType)?;
    let name = parser.parse_identifier()?;
    parser.cursor.expect(&Token::Assign)?;
    let ty = parser.parse_path_ty()?;
    let (_, end) = parser.cursor.expect(&Token::Semicolon)?;

    Ok((
        name,
        AssociatedTypeValue {
            ty,
            visibility,
            comments,
            location: parser.location(start, end),
        },
    ))
}

fn parse_trait_definition<'src, 'p, F, C>(
    parser: &mut super::ModuleParser<'src, 'p, F, C>,
    comments: Vec<Comment>,
    visibility: Visibility,
    start: usize,
) -> Result<DefinitionNode>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    parser.cursor.expect(&Token::KeywordTrait)?;
    let name = parser.parse_identifier()?;
    let generic_parameters = parser.parse_generic_parameters()?;
    parser.cursor.expect(&Token::LBrace)?;

    let mut associated_types = IndexMap::new();
    let mut body = Vec::new();
    let mut parsing_methods = false;
    let mut comments = comments;

    loop {
        let member_comments = parser.cursor.take_leading_comments();
        if parser.cursor.at(&Token::RBrace) {
            comments.extend(member_comments);
            break;
        }
        if parser.cursor.at_end() {
            return Err(Error::UnexpectedEof {
                expected: vec![ExpectedToken::Symbol("}".into())],
                location: parser.cursor.eof_location(),
            });
        }

        if starts_trait_associated_type(parser) {
            if parsing_methods {
                return Err(unexpected_item(parser));
            }
            let (name, associated_type) = parse_trait_associated_type(parser, member_comments)?;
            associated_types.insert(name, associated_type);
            continue;
        }

        parsing_methods = true;

        let member_start = member_comments
            .first()
            .map(Comment::location)
            .map(|location| location.start)
            .unwrap_or_else(|| parser.cursor.peek_start());
        let attrs = parse_attributes(parser)?;
        let member_visibility = parse_visibility(parser);
        let function = parse_function_definition(parser, member_comments, attrs, member_visibility, member_start, FunctionContext::Trait)?;
        body.push(parser.alloc_def(function));
    }

    let (_, end) = parser.cursor.expect(&Token::RBrace)?;
    Ok(DefinitionNode::Trait(TraitNode {
        name,
        associated_types,
        generic_parameters,
        body,
        visibility,
        comments,
        location: parser.location(start, end),
    }))
}

fn starts_trait_associated_type<'src, 'p, F, C>(parser: &super::ModuleParser<'src, 'p, F, C>) -> bool
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    matches!(
        (parser.cursor.lookahead(0), parser.cursor.lookahead(1)),
        (Some(Token::KeywordType), _) | (Some(Token::KeywordPub), Some(Token::KeywordType))
    )
}

fn parse_trait_associated_type<'src, 'p, F, C>(
    parser: &mut super::ModuleParser<'src, 'p, F, C>,
    comments: Vec<Comment>,
) -> Result<(Identifier, AssociatedType)>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    let start = comments
        .first()
        .map(Comment::location)
        .map(|location| location.start)
        .unwrap_or_else(|| parser.cursor.peek_start());
    let visibility = parse_visibility(parser);
    parser.cursor.expect(&Token::KeywordType)?;
    let generic = parse_one_generic_parameter(parser)?;
    let (_, end) = parser.cursor.expect(&Token::Semicolon)?;
    let name = Identifier::new(generic.name, generic.location);

    Ok((
        name,
        AssociatedType {
            constraints: generic.constraints,
            visibility,
            location: parser.location(start, end),
            comments,
        },
    ))
}

fn parse_one_generic_parameter<'src, 'p, F, C>(parser: &mut super::ModuleParser<'src, 'p, F, C>) -> Result<psy_ast::GenericParameter>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    let start = parser.cursor.peek_start();
    let name = parser.parse_identifier()?;
    let mut constraints = Vec::new();
    if parser.cursor.eat(&Token::Colon).is_some() {
        loop {
            constraints.push(parser.parse_path_ty()?);
            if parser.cursor.eat(&Token::OperatorAdd).is_none() {
                break;
            }
        }
    }
    let end = constraints
        .last()
        .map(UncheckedType::location)
        .map(|location| location.end)
        .unwrap_or(name.location.end);
    Ok(psy_ast::GenericParameter::new(name.id, constraints, parser.location(start, end)))
}

fn parse_type_alias_definition<'src, 'p, F, C>(
    parser: &mut super::ModuleParser<'src, 'p, F, C>,
    comments: Vec<Comment>,
    visibility: Visibility,
    start: usize,
) -> Result<DefinitionNode>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    parser.cursor.expect(&Token::KeywordType)?;
    let name = parser.parse_identifier()?;
    parser.cursor.expect(&Token::Assign)?;
    let ty = parser.parse_path_ty()?;
    let (_, end) = parser.cursor.expect(&Token::Semicolon)?;

    Ok(DefinitionNode::TypeAlias(TypeAliasNode {
        name,
        ty,
        visibility,
        comments,
        location: parser.location(start, end),
    }))
}

fn parse_attributes<'src, 'p, F, C>(parser: &mut super::ModuleParser<'src, 'p, F, C>) -> Result<Vec<AttrNode>>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    let mut attrs = Vec::new();
    while parser.cursor.at(&Token::Pound) {
        attrs.push(parse_attribute(parser)?);
    }
    Ok(attrs)
}

fn parse_attribute<'src, 'p, F, C>(parser: &mut super::ModuleParser<'src, 'p, F, C>) -> Result<AttrNode>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    let start = parser.cursor.expect(&Token::Pound)?.0;
    parser.cursor.expect(&Token::LBracket)?;

    let mut segments = vec![parser.parse_identifier()?];
    while parser.cursor.eat(&Token::DoubleColon).is_some() {
        segments.push(parser.parse_identifier()?);
    }
    let name = segments.pop().expect("an attribute always has a name");

    let mut properties = Vec::new();
    if parser.cursor.eat(&Token::LParen).is_some() {
        if !parser.cursor.at(&Token::RParen) {
            loop {
                properties.push(parser.parse_identifier()?);
                if parser.cursor.eat(&Token::Comma).is_none() || parser.cursor.at(&Token::RParen) {
                    break;
                }
            }
        }
        parser.cursor.expect(&Token::RParen)?;
    }

    let (_, end) = parser.cursor.expect(&Token::RBracket)?;
    Ok(AttrNode {
        path: segments,
        name,
        properties,
        location: parser.location(start, end),
    })
}

fn parse_visibility<'src, 'p, F, C>(parser: &mut super::ModuleParser<'src, 'p, F, C>) -> Visibility
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    if parser.cursor.eat(&Token::KeywordPub).is_some() {
        Visibility::Public
    } else {
        Visibility::Private
    }
}

fn unexpected_item<'src, 'p, F, C>(parser: &super::ModuleParser<'src, 'p, F, C>) -> Error
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    parser.unexpected(vec![ExpectedToken::Keyword("module item or definition")])
}
