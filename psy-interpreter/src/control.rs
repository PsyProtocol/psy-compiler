use std::{
    convert::Infallible,
    ops::{ControlFlow, FromResidual, Try},
};

use enum_as_inner::EnumAsInner;

#[derive(Clone, Debug, PartialEq, EnumAsInner)]
pub enum ControlState<T> {
    Normal(T),
    Return(T),
}

impl<T> Try for ControlState<T> {
    type Output = T;
    type Residual = Self;

    fn from_output(value: Self::Output) -> Self {
        ControlState::Normal(value)
    }

    fn branch(self) -> ControlFlow<Self::Residual, Self::Output> {
        match self {
            ControlState::Normal(value) => ControlFlow::Continue(value),
            other => ControlFlow::Break(other),
        }
    }
}

impl<T> FromResidual for ControlState<T> {
    fn from_residual(residual: <Self as Try>::Residual) -> Self {
        residual
    }
}
impl<T, E1, E2> FromResidual<Result<Infallible, E1>> for ControlState<Result<T, E2>>
where
    E1: Into<E2>,
{
    fn from_residual(residual: Result<Infallible, E1>) -> Self {
        match residual {
            Err(e) => ControlState::Return(Err(e.into())),
            Ok(infallible) => match infallible {},
        }
    }
}
impl<T> ControlState<T> {
    pub fn unwrap(self) -> T {
        match self {
            ControlState::Return(value) | ControlState::Normal(value) => value,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pipeline using both `?` forms: propagating an early `Return` state and
    /// converting a `Result` error into a `Return` state.
    fn pipeline(control: ControlState<Result<u32, String>>) -> ControlState<Result<u32, String>> {
        let result = control?;
        let value = result?;
        ControlState::from_output(Ok(value + 1))
    }

    #[test]
    fn try_trait_continues_from_normal_output() {
        assert_eq!(pipeline(ControlState::Normal(Ok(41))), ControlState::Normal(Ok(42)));
    }

    #[test]
    fn try_trait_propagates_early_returns_unchanged() {
        let early = ControlState::Return(Err("early".to_string()));
        assert_eq!(pipeline(early.clone()), early);
    }

    #[test]
    fn try_trait_converts_result_errors_into_returns() {
        assert_eq!(
            pipeline(ControlState::Normal(Err("boom".to_string()))),
            ControlState::Return(Err("boom".to_string()))
        );
    }

    #[test]
    fn unwrap_yields_the_payload_of_both_variants() {
        assert_eq!(ControlState::Normal(7).unwrap(), 7);
        assert_eq!(ControlState::Return(8).unwrap(), 8);
    }

    #[test]
    fn enum_accessors_distinguish_the_variants() {
        let normal = ControlState::Normal(1);
        let ret = ControlState::Return(2);
        assert!(normal.is_normal() && !normal.is_return());
        assert!(ret.is_return() && !ret.is_normal());
        assert_eq!(normal.as_normal(), Some(&1));
        assert_eq!(normal.as_return(), None);
        assert_eq!(ret.as_return(), Some(&2));
        assert_eq!(ret.as_normal(), None);
    }

    #[test]
    fn mutable_and_consuming_accessors_round_trip() {
        let mut normal = ControlState::Normal(1);
        if let Some(value) = normal.as_normal_mut() {
            *value += 1;
        }
        assert_eq!(normal.as_normal(), Some(&2));

        let mut ret = ControlState::Return(3);
        if let Some(value) = ret.as_return_mut() {
            *value *= 2;
        }
        assert_eq!(ret.as_return(), Some(&6));

        assert_eq!(ControlState::Normal(4).into_normal(), Ok(4));
        assert_eq!(ControlState::Normal(4).into_return(), Err(ControlState::Normal(4)));
        assert_eq!(ControlState::Return(5).into_return(), Ok(5));
        assert_eq!(ControlState::Return(5).into_normal(), Err(ControlState::Return(5)));
    }
}
