//! Type parsing for the recursive-descent parser.
//!
//! Handles: basic types, generic types, array types, tuple types,
//! function signatures, path types, trait casts, and const values.

use psy_ast::{ConstValue, FunctionSignature, Identifier, Location, PathNode, UncheckedType};
use psy_lexer::Token;

use crate::{
    error::{Error, Result},
    recursive::cursor::TokenCursor,
};

impl<'src, 'p, F, C> super::ModuleParser<'src, 'p, F, C>
where
    F: Clone + From<u32>,
{
    /// Parse a type (PathTy in LALRPOP grammar).
    ///
    /// Supports turbofish generic args on path segments (`mod::<T>::Fn`) in
    /// addition to the bare `mod::Fn<T>` form. The closing `>` uses contextual
    /// `>>` splitting so nested non-empty generics (`A<B<C>>`) keep working.
    pub fn parse_path_ty(&mut self) -> Result<UncheckedType> {
        let start = self.cursor.peek_start();

        // Parse leading segments: (BasicIdentTy ("::" ("<" GenericArgs>)?)*
        let mut segments: Vec<UncheckedType> = Vec::new();
        let target;
        loop {
            let cp = self.cursor.checkpoint();
            let component = match self.parse_basic_ident_or_generic_ty() {
                Ok(seg) => seg,
                Err(_) => {
                    self.cursor.rewind(cp);
                    target = self.parse_ty()?;
                    break;
                }
            };

            if self.cursor.eat(&Token::DoubleColon).is_none() {
                // `component` is the target; rewind so `parse_ty` re-parses it
                // and can apply bare `<T>` generic args.
                self.cursor.rewind(cp);
                target = self.parse_ty()?;
                break;
            }

            // `::` consumed. Turbofish `::<generics>` on this segment?
            if self.cursor.at(&Token::OperatorLt) {
                self.cursor.advance(); // consume `<`
                let generics = self.parse_generic_args()?;
                let seg = self.apply_generic_args(&component, generics)?;
                if self.cursor.eat(&Token::DoubleColon).is_some() {
                    segments.push(seg);
                    continue;
                }
                // `seg::<generics>` with no following `::` is the target.
                target = seg;
                break;
            }

            segments.push(component);
        }

        let end = self.cursor.peek_start();

        if segments.is_empty() {
            Ok(target)
        } else {
            let root = segments.first().cloned();
            let rest: Vec<UncheckedType> = segments.into_iter().skip(1).collect();
            Ok(UncheckedType::Path(Box::new(PathNode {
                root,
                segments: rest,
                target,
                is_ty: true,
                location: self.location(start, end),
            })))
        }
    }

    /// Attach turbofish generic arguments to an identifier path segment.
    pub(super) fn apply_generic_args(&self, component: &UncheckedType, generics: Vec<UncheckedType>) -> Result<UncheckedType> {
        match component {
            UncheckedType::Basic(ident) => {
                let end = self.cursor.peek_start();
                Ok(UncheckedType::Generic(*ident, generics, self.location(ident.location.start, end)))
            }
            _ => Err(Error::UnsupportedSyntax {
                feature: "generic arguments on non-identifier path segment".into(),
                location: component.location(),
            }),
        }
    }

    /// Parse a basic type: Felt, Bool, u32, Self, or an identifier.
    fn parse_basic_type(&mut self) -> Result<UncheckedType> {
        let start = self.cursor.peek_start();
        let end = self.cursor.peek_end();
        let loc = self.location(start, end);

        match self.cursor.peek() {
            Some(Token::TypeFelt) => {
                self.cursor.advance();
                Ok(UncheckedType::Basic(Identifier::new(psy_ast::IdentId::TYPE_FELT, loc)))
            }
            Some(Token::TypeBool) => {
                self.cursor.advance();
                Ok(UncheckedType::Basic(Identifier::new(psy_ast::IdentId::TYPE_BOOL, loc)))
            }
            Some(Token::TypeU32) => {
                self.cursor.advance();
                Ok(UncheckedType::Basic(Identifier::new(psy_ast::IdentId::TYPE_U32, loc)))
            }
            Some(Token::TypeSelf) => {
                self.cursor.advance();
                Ok(UncheckedType::Basic(Identifier::new(psy_ast::IdentId::TYPE_SELF, loc)))
            }
            Some(Token::Ident(name)) => {
                let name = name.to_string();
                self.cursor.advance();
                Ok(UncheckedType::Basic(self.ident(&name, loc)))
            }
            _ => Err(Error::UnexpectedToken {
                found: format!("{:?}", self.cursor.peek()),
                expected: vec![crate::error::ExpectedToken::Type],
                location: self.cursor.current_location(),
            }),
        }
    }

    /// Parse a Ty: basic, array, tuple, generic, or function signature.
    fn parse_ty(&mut self) -> Result<UncheckedType> {
        match self.cursor.peek() {
            Some(Token::KeywordFn) => self.parse_function_signature_type(),
            Some(Token::LBracket) => self.parse_array_or_generic_array_type(),
            Some(Token::LParen) => self.parse_tuple_type(),
            _ => {
                let start = self.cursor.peek_start();
                let basic = self.parse_basic_type()?;

                if let UncheckedType::Basic(ident) = &basic {
                    if self.cursor.at(&Token::OperatorLt) {
                        self.cursor.advance();
                        let generics = self.parse_generic_args()?;
                        let end = self.cursor.peek_start();
                        return Ok(UncheckedType::Generic(ident.clone(), generics, self.location(start, end)));
                    }
                }

                Ok(basic)
            }
        }
    }

    /// Parse array type: [PathTy; U64] or [PathTy; Ident] (generic array)
    fn parse_array_or_generic_array_type(&mut self) -> Result<UncheckedType> {
        let start = self.cursor.peek_start();
        self.cursor.expect(&Token::LBracket)?;
        let elem_ty = self.parse_path_ty()?;
        self.cursor.expect(&Token::Semicolon)?;

        let end_start = self.cursor.peek_start();
        match self.cursor.peek() {
            Some(Token::U64(n)) => {
                let n = *n;
                self.cursor.advance();
                self.cursor.expect(&Token::RBracket)?;
                let end = self.cursor.peek_start();
                Ok(UncheckedType::Array(
                    Box::new(elem_ty),
                    ConstValue::Felt(n),
                    self.location(start, end),
                ))
            }
            Some(Token::Ident(name)) => {
                let name = name.to_string();
                self.cursor.advance();
                self.cursor.expect(&Token::RBracket)?;
                let end = self.cursor.peek_start();
                // [T; N] where N is a named constant → Generic(Array, [T, N])
                Ok(UncheckedType::Generic(
                    Identifier::new(psy_ast::IdentId::TYPE_ARRAY, self.location(start, end)),
                    vec![elem_ty, UncheckedType::Basic(self.ident(&name, self.location(end_start, end)))],
                    self.location(start, end),
                ))
            }
            _ => Err(Error::UnexpectedToken {
                found: format!("{:?}", self.cursor.peek()),
                expected: vec![crate::error::ExpectedToken::Literal, crate::error::ExpectedToken::Ident],
                location: self.cursor.current_location(),
            }),
        }
    }

    /// Parse tuple type: (PathTy, PathTy, ...)
    fn parse_tuple_type(&mut self) -> Result<UncheckedType> {
        let start = self.cursor.peek_start();
        self.cursor.expect(&Token::LParen)?;
        let mut types = Vec::new();

        if !self.cursor.at(&Token::RParen) {
            loop {
                types.push(self.parse_path_ty()?);
                if self.cursor.eat(&Token::Comma).is_none() {
                    break;
                }
                if self.cursor.at(&Token::RParen) {
                    break; // trailing comma
                }
            }
        }

        let end = self.cursor.expect(&Token::RParen)?.1;
        Ok(UncheckedType::Tuple(types, self.location(start, end)))
    }

    /// Parse generic type arguments: <Type1, Type2, ...>
    /// Consumes the closing `>` (with contextual >> splitting).
    pub(super) fn parse_generic_args(&mut self) -> Result<Vec<UncheckedType>> {
        let mut args = Vec::new();

        // Reject empty generic argument lists (`<>`, `::<>`). A split `>>`
        // closer for an inner empty generic is also rejected; non-empty
        // nested generics (`A<B<C>>`) still parse via the loop below.
        let closer_start = self.cursor.peek_start();
        if self.cursor.eat_type_gt() {
            return Err(Error::UnsupportedSyntax {
                feature: "empty generic argument list".into(),
                location: self.cursor.location(closer_start, closer_start + 1),
            });
        }

        loop {
            args.push(self.parse_monomorphization_ty()?);
            if self.cursor.eat(&Token::Comma).is_none() {
                break;
            }
            // Trailing comma followed by the closer: `A<T,>`.
            if self.cursor.eat_type_gt() {
                return Ok(args);
            }
        }

        if !self.cursor.eat_type_gt() {
            return Err(Error::UnexpectedToken {
                found: format!("{:?}", self.cursor.peek()),
                expected: vec![crate::error::ExpectedToken::Symbol(">".into())],
                location: self.cursor.current_location(),
            });
        }
        Ok(args)
    }

    /// Parse a monomorphization type (const value or path type)
    fn parse_monomorphization_ty(&mut self) -> Result<UncheckedType> {
        let start = self.cursor.peek_start();
        match self.cursor.peek() {
            Some(Token::U64(n)) => {
                let n = *n;
                self.cursor.advance();
                let end = self.cursor.peek_start();
                Ok(UncheckedType::Const(ConstValue::Felt(n), self.location(start, end)))
            }
            Some(Token::U32(n)) => {
                let n = *n;
                self.cursor.advance();
                let end = self.cursor.peek_start();
                Ok(UncheckedType::Const(ConstValue::U32(n), self.location(start, end)))
            }
            Some(Token::Bool(b)) => {
                let b = *b;
                self.cursor.advance();
                let end = self.cursor.peek_start();
                Ok(UncheckedType::Const(ConstValue::Bool(b), self.location(start, end)))
            }
            _ => self.parse_path_ty(),
        }
    }

    /// Parse basic ident type (for path segments): BasicType or `self`
    pub fn parse_basic_ident_ty(&mut self) -> Result<UncheckedType> {
        match self.cursor.peek() {
            Some(Token::KeywordSelf) => {
                let start = self.cursor.peek_start();
                let end = self.cursor.peek_end();
                self.cursor.advance();
                Ok(UncheckedType::Basic(Identifier::new(psy_ast::IdentId::SELF, self.location(start, end))))
            }
            _ => self.parse_basic_type(),
        }
    }

    /// Parse basic ident or generic type (for path expression segments)
    pub fn parse_basic_ident_or_generic_ty(&mut self) -> Result<UncheckedType> {
        // Check for <Type> or <Type as Trait> syntax
        if self.cursor.at(&Token::OperatorLt) {
            let start = self.cursor.peek_start();
            self.cursor.advance(); // consume <
            let inner_ty = self.parse_path_ty()?;

            if self.cursor.eat(&Token::KeywordAs).is_some() {
                let trait_ty = self.parse_path_ty()?;
                self.cursor.expect_type_gt()?;
                let end = self.cursor.peek_start();
                return Ok(UncheckedType::TraitCast(
                    Box::new(inner_ty),
                    Box::new(trait_ty),
                    self.location(start, end),
                ));
            }

            self.cursor.expect_type_gt()?;
            return Ok(inner_ty);
        }

        self.parse_basic_ident_ty()
    }

    /// Parse a function return type: -> PathTy
    pub fn parse_function_return(&mut self) -> Result<Option<UncheckedType>> {
        if self.cursor.eat(&Token::Arrow).is_some() {
            Ok(Some(self.parse_path_ty()?))
        } else {
            Ok(None)
        }
    }

    /// Parse a function signature type: fn(PathTy, ...) -> PathTy
    pub fn parse_function_signature_type(&mut self) -> Result<UncheckedType> {
        let start = self.cursor.peek_start();
        self.cursor.expect(&Token::KeywordFn)?;
        self.cursor.expect(&Token::LParen)?;

        let mut params = Vec::new();
        if !self.cursor.at(&Token::RParen) {
            loop {
                params.push(self.parse_path_ty()?);
                if self.cursor.eat(&Token::Comma).is_none() {
                    break;
                }
            }
        }
        self.cursor.expect(&Token::RParen)?;

        let return_type = self.parse_function_return()?;
        let end = self.cursor.peek_start();

        Ok(UncheckedType::FunctionSignature(
            Box::new(FunctionSignature {
                parameters: params,
                return_type,
            }),
            self.location(start, end),
        ))
    }

    /// Parse a const type (used in const declarations): Felt, Bool, u32
    pub fn parse_const_type(&mut self) -> Result<UncheckedType> {
        let start = self.cursor.peek_start();
        let end = self.cursor.peek_end();
        let loc = self.location(start, end);

        match self.cursor.peek() {
            Some(Token::TypeFelt) => {
                self.cursor.advance();
                Ok(UncheckedType::Basic(Identifier::new(psy_ast::IdentId::TYPE_FELT, loc)))
            }
            Some(Token::TypeBool) => {
                self.cursor.advance();
                Ok(UncheckedType::Basic(Identifier::new(psy_ast::IdentId::TYPE_BOOL, loc)))
            }
            Some(Token::TypeU32) => {
                self.cursor.advance();
                Ok(UncheckedType::Basic(Identifier::new(psy_ast::IdentId::TYPE_U32, loc)))
            }
            Some(Token::TypeSelf) => {
                self.cursor.advance();
                Ok(UncheckedType::Basic(Identifier::new(psy_ast::IdentId::TYPE_SELF, loc)))
            }
            Some(Token::Ident(name)) => {
                let name = name.to_string();
                self.cursor.advance();
                Ok(UncheckedType::Basic(self.ident(&name, loc)))
            }
            _ => self.parse_path_ty(),
        }
    }

    /// Parse generic parameters: <T, N: u32, ...>
    pub fn parse_generic_parameters(&mut self) -> Result<Vec<psy_ast::GenericParameter>> {
        if !self.cursor.at(&Token::OperatorLt) {
            return Ok(vec![]);
        }

        let mut params = Vec::new();
        self.cursor.advance(); // consume <

        loop {
            let closer_start = self.cursor.peek_start();
            if self.cursor.eat_type_gt() {
                if params.is_empty() {
                    return Err(Error::UnsupportedSyntax {
                        feature: "empty generic parameter list".into(),
                        location: self.cursor.location(closer_start, closer_start + 1),
                    });
                }
                break;
            }

            let start = self.cursor.peek_start();
            let name = self.parse_identifier()?;
            let constraints = self.parse_generic_constraints()?;
            let end = self.cursor.peek_start();

            params.push(psy_ast::GenericParameter::new(name.id, constraints, self.location(start, end)));

            if self.cursor.eat(&Token::Comma).is_none() {
                self.cursor.expect_type_gt()?;
                break;
            }
        }

        Ok(params)
    }

    /// Parse generic constraints: : Trait1 + Trait2
    fn parse_generic_constraints(&mut self) -> Result<Vec<UncheckedType>> {
        if self.cursor.eat(&Token::Colon).is_none() {
            return Ok(vec![]);
        }

        let mut constraints = Vec::new();
        loop {
            constraints.push(self.parse_path_ty()?);
            if self.cursor.eat(&Token::OperatorAdd).is_none() {
                break;
            }
        }
        Ok(constraints)
    }

    /// Parse an identifier (interning into the program's interner).
    pub fn parse_identifier(&mut self) -> Result<Identifier> {
        let start = self.cursor.peek_start();
        let end = self.cursor.peek_end();
        let loc = self.location(start, end);

        match self.cursor.peek() {
            Some(Token::Ident(name)) => {
                let name = name.to_string();
                self.cursor.advance();
                Ok(self.ident(&name, loc))
            }
            _ => Err(Error::UnexpectedToken {
                found: format!("{:?}", self.cursor.peek()),
                expected: vec![crate::error::ExpectedToken::Ident],
                location: self.cursor.current_location(),
            }),
        }
    }

    /// Parse an identifier or `self` keyword.
    pub fn parse_identifier_or_self(&mut self) -> Result<Identifier> {
        match self.cursor.peek() {
            Some(Token::KeywordSelf) => {
                let start = self.cursor.peek_start();
                let end = self.cursor.peek_end();
                self.cursor.advance();
                Ok(Identifier::new(psy_ast::IdentId::SELF, self.location(start, end)))
            }
            _ => self.parse_identifier(),
        }
    }
}
