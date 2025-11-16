use std::{
    ops::{Add, AddAssign, Mul, MulAssign, Sub, SubAssign},
    vec,
};
mod ex1;
mod ex2;
pub mod ex3;
pub mod ex5;
pub mod ex6;
pub mod ex7;
pub mod ex8;
pub mod tok;
use ex3::ex3_generic::{test_it_3, test_it_3v2};
use ex6::SimpleContractState;
use psy_vm::dpn::ops::{context_trait::FeltSized, exec_context::QExecContext};
use psylang_macros::show_streams;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub enum OpType {
    Input = 0,
    Constant = 1,
    Add = 2,
    Sub = 3,
    Mul = 4,
    Select = 5,
    Eq = 6,
    Neq = 7,
    Lt = 8,
    Lte = 9,
    Gt = 10,
    Gte = 11,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FeltOp {
    pub op_type: OpType,
    pub args: Vec<FeltOp>,
    pub params: Vec<u64>,
}

impl FeltOp {
    pub fn new(op_type: OpType, args: Vec<FeltOp>, params: Vec<u64>) -> FeltOp {
        FeltOp { op_type, args, params }
    }
    pub fn new_input(index: u64) -> FeltOp {
        FeltOp {
            op_type: OpType::Input,
            args: vec![],
            params: vec![index],
        }
    }
    pub fn new_constant(value: u64) -> FeltOp {
        FeltOp {
            op_type: OpType::Constant,
            args: vec![],
            params: vec![value],
        }
    }
    pub fn new_select(condition: FeltOp, if_true: FeltOp, if_false: FeltOp) -> FeltOp {
        FeltOp {
            op_type: OpType::Select,
            args: vec![condition, if_true, if_false],
            params: vec![],
        }
    }
    pub fn new_add(a: FeltOp, b: FeltOp) -> FeltOp {
        FeltOp {
            op_type: OpType::Add,
            args: vec![a, b],
            params: vec![],
        }
    }
    pub fn new_sub(a: FeltOp, b: FeltOp) -> FeltOp {
        FeltOp {
            op_type: OpType::Sub,
            args: vec![a, b],
            params: vec![],
        }
    }
    pub fn new_mul(a: FeltOp, b: FeltOp) -> FeltOp {
        FeltOp {
            op_type: OpType::Mul,
            args: vec![a, b],
            params: vec![],
        }
    }
    pub fn new_eq(a: FeltOp, b: FeltOp) -> FeltOp {
        FeltOp {
            op_type: OpType::Eq,
            args: vec![a, b],
            params: vec![],
        }
    }
    pub fn new_neq(a: FeltOp, b: FeltOp) -> FeltOp {
        FeltOp {
            op_type: OpType::Neq,
            args: vec![a, b],
            params: vec![],
        }
    }
    pub fn new_lt(a: FeltOp, b: FeltOp) -> FeltOp {
        FeltOp {
            op_type: OpType::Lt,
            args: vec![a, b],
            params: vec![],
        }
    }
    pub fn new_lte(a: FeltOp, b: FeltOp) -> FeltOp {
        FeltOp {
            op_type: OpType::Lte,
            args: vec![a, b],
            params: vec![],
        }
    }
    pub fn new_gt(a: FeltOp, b: FeltOp) -> FeltOp {
        FeltOp {
            op_type: OpType::Gt,
            args: vec![a, b],
            params: vec![],
        }
    }
    pub fn new_gte(a: FeltOp, b: FeltOp) -> FeltOp {
        FeltOp {
            op_type: OpType::Gte,
            args: vec![a, b],
            params: vec![],
        }
    }
}

impl From<u32> for FeltOp {
    fn from(value: u32) -> FeltOp {
        FeltOp::new_constant(value as u64)
    }
}
impl From<u64> for FeltOp {
    fn from(value: u64) -> FeltOp {
        FeltOp::new_constant(value)
    }
}
impl From<u8> for FeltOp {
    fn from(value: u8) -> FeltOp {
        FeltOp::new_constant(value as u64)
    }
}
impl From<u16> for FeltOp {
    fn from(value: u16) -> FeltOp {
        FeltOp::new_constant(value as u64)
    }
}
impl From<bool> for FeltOp {
    fn from(value: bool) -> FeltOp {
        FeltOp::new_constant(if value { 1 } else { 0 })
    }
}

impl Add for FeltOp {
    type Output = FeltOp;

    fn add(self, other: FeltOp) -> FeltOp {
        FeltOp::new(OpType::Add, vec![self, other], vec![])
    }
}
impl Mul for FeltOp {
    type Output = FeltOp;

    fn mul(self, other: FeltOp) -> FeltOp {
        FeltOp::new(OpType::Mul, vec![self, other], vec![])
    }
}
impl Sub for FeltOp {
    type Output = FeltOp;

    fn sub(self, other: FeltOp) -> FeltOp {
        FeltOp::new(OpType::Sub, vec![self, other], vec![])
    }
}
impl AddAssign for FeltOp {
    fn add_assign(&mut self, other: FeltOp) {
        *self = FeltOp::new(OpType::Add, vec![self.clone(), other], vec![]);
    }
}
impl MulAssign for FeltOp {
    fn mul_assign(&mut self, other: FeltOp) {
        *self = FeltOp::new(OpType::Mul, vec![self.clone(), other], vec![]);
    }
}
impl SubAssign for FeltOp {
    fn sub_assign(&mut self, other: FeltOp) {
        *self = FeltOp::new(OpType::Sub, vec![self.clone(), other], vec![]);
    }
}

fn test_function(x: FeltOp, y: FeltOp) -> FeltOp {
    let mut a = x + y;

    let c = a;
    c
}

fn main() {
    let simple_size = SimpleContractState::size();
    println!("SimpleContractState size: {}", simple_size);
    let mut ctx = QExecContext::new();
    ex3::test_contract();
    test_it_3();
    test_it_3v2();
}
