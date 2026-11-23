//! Checked unsigned N*32-bit arithmetic expanded into existing scalar DPN operations.
//!
//! `N == 1` (the u32 lane) emits the native single-limb `op_u32_*` opcodes so the
//! op graph matches the u32 paths that predate this type; `N >= 2` uses the
//! multi-limb carry/borrow expansion introduced for u256. No new opcodes and no
//! new `DPNContext` methods — every operation decomposes into existing scalar ops.
use std::{fmt, str::FromStr};
use psy_vm::dpn::ops::context_trait::{ContextFelt, DPNContext};
use psy_vm::dpn::ops::op_types::DPNOpType;

/// N base-2^32 digits, least significant first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Uint<const N: usize>(pub [u32; N]);

pub type U64 = Uint<2>;
pub type U128 = Uint<4>;
pub type U256 = Uint<8>;

impl<const N: usize> FromStr for Uint<N> {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let name = format!("u{}", 32 * N);
        let (digits, radix) = match value.strip_prefix("0x").or_else(|| value.strip_prefix("0X")) {
            Some(digits) => (digits, 16u64),
            None => (value, 10u64),
        };
        if digits.is_empty() { return Err(format!("empty {name} literal")); }
        let mut limbs = [0u32; N];
        for digit in digits.chars() {
            let mut carry = digit.to_digit(radix as u32).ok_or("invalid uint digit")? as u64;
            for limb in &mut limbs {
                let product = *limb as u64 * radix + carry;
                *limb = product as u32;
                carry = product >> 32;
            }
            if carry != 0 { return Err(format!("{name} literal overflow")); }
        }
        Ok(Self(limbs))
    }
}

impl<const N: usize> fmt::Display for Uint<N> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut limbs = self.0;
        let mut digits = Vec::new();
        while limbs.iter().any(|limb| *limb != 0) {
            let mut remainder = 0u64;
            for limb in limbs.iter_mut().rev() {
                let value = (remainder << 32) | *limb as u64;
                *limb = (value / 10) as u32;
                remainder = value % 10;
            }
            digits.push((b'0' + remainder as u8) as char);
        }
        if digits.is_empty() { return formatter.write_str("0"); }
        for digit in digits.iter().rev() { write!(formatter, "{digit}")?; }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct UintValue<F: ContextFelt, const N: usize>(pub [F; N]);

pub type U64Value<F> = UintValue<F, 2>;
pub type U128Value<F> = UintValue<F, 4>;
pub type U256Value<F> = UintValue<F, 8>;

impl<F: ContextFelt, const N: usize> UintValue<F, N> {
    fn is_constant<C: DPNContext<F>>(ctx: &C, value: F) -> bool {
        matches!(ctx.get_op_type(value), DPNOpType::Constant | DPNOpType::ConstantU32 | DPNOpType::ConstantTrue | DPNOpType::ConstantFalse)
    }
    fn split_bits<C: DPNContext<F>>(ctx: &mut C, value: F, width: u64) -> Vec<F> {
        if Self::is_constant(ctx, value) {
            let value = ctx.get_constant_value(value);
            return (0..width).map(|bit| if (value >> bit) & 1 == 0 { ctx.op_false() } else { ctx.op_true() }).collect();
        }
        ctx.split_bits(value, width)
    }
    fn sum_bits<C: DPNContext<F>>(ctx: &mut C, bits: &[F]) -> F {
        if bits.iter().all(|bit| Self::is_constant(ctx, *bit)) {
            let value = bits.iter().enumerate().fold(0u32, |value, (i, bit)| value | ((ctx.get_constant_value(*bit) as u32) << i));
            return ctx.op_const_u32(value);
        }
        ctx.sum_bits(bits)
    }
    fn felt_add<C: DPNContext<F>>(ctx: &mut C, a: F, b: F) -> F {
        let a = ctx.op_cast_felt(a);
        let b = ctx.op_cast_felt(b);
        ctx.op_add(a, b)
    }
    fn felt_sub<C: DPNContext<F>>(ctx: &mut C, a: F, b: F) -> F {
        let a = ctx.op_cast_felt(a);
        let b = ctx.op_cast_felt(b);
        ctx.op_sub(a, b)
    }
    fn felt_mul<C: DPNContext<F>>(ctx: &mut C, a: F, b: F) -> F {
        let a = ctx.op_cast_felt(a);
        let b = ctx.op_cast_felt(b);
        ctx.op_mul(a, b)
    }
    pub fn from_limbs<C: DPNContext<F>>(ctx: &mut C, limbs: [F; N]) -> Self {
        Self(limbs.map(|limb| ctx.op_cast_u32(limb)))
    }
    pub fn constant<C: DPNContext<F>>(ctx: &mut C, value: Uint<N>) -> Self {
        Self(value.0.map(|limb| ctx.op_const_u32(limb)))
    }
    pub fn from_felt<C: DPNContext<F>>(ctx: &mut C, value: F) -> Self {
        if N == 1 {
            // Native lane: CastU32 range-checks the value into a single limb.
            return Self(std::array::from_fn(|_| ctx.op_cast_u32(value)));
        }
        let (low, high) = Self::split(ctx, value, 64);
        let zero = ctx.op_const_u32(0);
        let mut limbs = [zero; N];
        limbs[0] = low;
        if N > 1 { limbs[1] = high; }
        Self::from_limbs(ctx, limbs)
    }
    pub fn to_u32<C: DPNContext<F>>(self, ctx: &mut C) -> F {
        let value = Self::from_limbs(ctx, self.0);
        let zero = ctx.op_const_u32(0);
        for limb in &value.0[1..] { ctx.assert_eq(*limb, zero, "uint to u32 overflow"); }
        value.0[0]
    }
    pub fn to_felt<C: DPNContext<F>>(self, ctx: &mut C) -> F {
        if N == 1 {
            return ctx.op_cast_felt(self.0[0]);
        }
        let value = Self::from_limbs(ctx, self.0);
        let zero = ctx.op_const_u32(0);
        for limb in &value.0[2..] { ctx.assert_eq(*limb, zero, "uint to felt overflow"); }
        // p = (u32::MAX << 32) + 1: only low=0 is canonical at high=u32::MAX.
        let max = ctx.op_const_u32(u32::MAX);
        let high_max = ctx.op_eq(value.0[1], max);
        let low_nonzero = ctx.op_neq(value.0[0], zero);
        let invalid = ctx.op_bool_and(high_max, low_nonzero);
        ctx.assert_eq(invalid, zero, "uint to felt overflow");
        let base = ctx.op_const(1u64 << 32);
        let high = Self::felt_mul(ctx, value.0[1], base);
        Self::felt_add(ctx, high, value.0[0])
    }
    fn split<C: DPNContext<F>>(ctx: &mut C, value: F, width: u64) -> (F, F) {
        let bits = Self::split_bits(ctx, value, width);
        let low = Self::sum_bits(ctx, &bits[..32]);
        let high = Self::sum_bits(ctx, &bits[32..]);
        if width == 64 {
            let max = ctx.op_const_u32(u32::MAX);
            let zero = ctx.op_const_u32(0);
            let high_max = ctx.op_eq(high, max);
            let low_nonzero = ctx.op_neq(low, zero);
            let invalid = ctx.op_bool_and(high_max, low_nonzero);
            ctx.assert_eq(invalid, zero, "uint noncanonical 64-bit decomposition");
        }
        (ctx.op_cast_u32(low), ctx.op_cast_u32(high))
    }
    /// Slice-based so div_rem can drive it on N+1 scratch limbs (stable Rust
    /// cannot express `[F; N + 1]` arrays generically).
    fn sub_limbs<C: DPNContext<F>>(ctx: &mut C, a: &[F], b: &[F]) -> (Vec<F>, F) {
        let zero = ctx.op_const_u32(0);
        let base = ctx.op_const(1u64 << 32);
        let one = ctx.op_const_u32(1);
        let mut borrow = zero;
        let mut result = Vec::with_capacity(a.len());
        for i in 0..a.len() {
            // 0 <= B+a-b-borrow <= 2B-1; never a field wrap.
            let value = Self::felt_add(ctx, base, a[i]);
            let value = Self::felt_sub(ctx, value, b[i]);
            let value = Self::felt_sub(ctx, value, borrow);
            let (low, high) = Self::split(ctx, value, 33);
            borrow = Self::felt_sub(ctx, one, high);
            result.push(low);
        }
        (result, borrow)
    }
    pub fn add<C: DPNContext<F>>(self, ctx: &mut C, other: Self) -> Self {
        if N == 1 {
            return Self(std::array::from_fn(|_| ctx.op_u32_add(self.0[0], other.0[0])));
        }
        let a = Self::from_limbs(ctx, self.0);
        let b = Self::from_limbs(ctx, other.0);
        let zero = ctx.op_const_u32(0);
        let mut carry = zero;
        let limbs = std::array::from_fn(|i| {
            let value = Self::felt_add(ctx, a.0[i], b.0[i]);
            let value = Self::felt_add(ctx, value, carry);
            let (low, high) = Self::split(ctx, value, 33);
            carry = high;
            low
        });
        ctx.assert_eq(carry, zero, "uint addition overflow");
        Self(limbs)
    }
    pub fn sub<C: DPNContext<F>>(self, ctx: &mut C, other: Self) -> Self {
        if N == 1 {
            return Self(std::array::from_fn(|_| ctx.op_u32_sub(self.0[0], other.0[0])));
        }
        let a = Self::from_limbs(ctx, self.0);
        let b = Self::from_limbs(ctx, other.0);
        let (limbs, borrow) = Self::sub_limbs(ctx, &a.0, &b.0);
        let zero = ctx.op_const_u32(0);
        ctx.assert_eq(borrow, zero, "uint subtraction underflow");
        Self(std::array::from_fn(|i| limbs[i]))
    }
    pub fn mul<C: DPNContext<F>>(self, ctx: &mut C, other: Self) -> Self {
        if N == 1 {
            return Self(std::array::from_fn(|_| ctx.op_u32_mul(self.0[0], other.0[0])));
        }
        let a = Self::from_limbs(ctx, self.0);
        let b = Self::from_limbs(ctx, other.0);
        let zero = ctx.op_const_u32(0);
        let mut limbs = vec![zero; 2 * N];
        for i in 0..N {
            let mut carry = zero;
            for j in 0..N {
                // Split each product before accumulation: product+two carries could exceed p.
                let product = Self::felt_mul(ctx, a.0[i], b.0[j]);
                let (low, high) = Self::split(ctx, product, 64);
                let sum = Self::felt_add(ctx, limbs[i+j], low);
                let sum = Self::felt_add(ctx, sum, carry);
                let (low, extra) = Self::split(ctx, sum, 34);
                limbs[i+j] = low;
                carry = Self::felt_add(ctx, high, extra);
                carry = ctx.op_cast_u32(carry);
            }
            limbs[i+N] = carry;
        }
        for limb in &limbs[N..] { ctx.assert_eq(*limb, zero, "uint multiplication overflow"); }
        Self(std::array::from_fn(|i| limbs[i]))
    }
    pub fn eq<C: DPNContext<F>>(self, ctx: &mut C, other: Self) -> F {
        if N == 1 {
            return ctx.op_eq(self.0[0], other.0[0]);
        }
        let a = Self::from_limbs(ctx, self.0);
        let b = Self::from_limbs(ctx, other.0);
        let equal: Vec<_> = (0..N).map(|i| ctx.op_eq(a.0[i], b.0[i])).collect();
        ctx.op_bool_and_many(&equal)
    }
    pub fn lt<C: DPNContext<F>>(self, ctx: &mut C, other: Self) -> F {
        if N == 1 {
            return ctx.op_lt(self.0[0], other.0[0]);
        }
        let a = Self::from_limbs(ctx, self.0);
        let b = Self::from_limbs(ctx, other.0);
        Self::sub_limbs(ctx, &a.0, &b.0).1
    }
    pub fn div_rem<C: DPNContext<F>>(self, ctx: &mut C, other: Self) -> (Self, Self) {
        if N == 1 {
            // Native lane: U32Div/U32Mod carry their own zero-divisor check.
            let quotient = ctx.op_u32_div(self.0[0], other.0[0]);
            let remainder = ctx.op_u32_mod(self.0[0], other.0[0]);
            return (Self(std::array::from_fn(|_| quotient)), Self(std::array::from_fn(|_| remainder)));
        }
        let a = Self::from_limbs(ctx, self.0);
        let b = Self::from_limbs(ctx, other.0);
        let zero = ctx.op_const_u32(0);
        let is_zero = b.eq(ctx, Self([zero; N]));
        ctx.assert_eq(is_zero, zero, "uint division by zero");
        let bits: Vec<_> = a.0.iter().flat_map(|limb| Self::split_bits(ctx, *limb, 32)).collect();
        let mut remainder = vec![zero; N + 1];
        let mut divisor = vec![zero; N + 1];
        divisor[..N].copy_from_slice(&b.0);
        let mut quotient = vec![zero; 32 * N];
        for i in (0..32 * N).rev() {
            let mut carry = bits[i];
            for limb in remainder.iter_mut() {
                let doubled = Self::felt_add(ctx, *limb, *limb);
                let shifted = Self::felt_add(ctx, doubled, carry);
                let (low, high) = Self::split(ctx, shifted, 33);
                *limb = low;
                carry = high;
            }
            let (difference, borrow) = Self::sub_limbs(ctx, &remainder, &divisor);
            let take = ctx.op_bool_not(borrow);
            for j in 0..N + 1 { remainder[j] = ctx.op_select(take, difference[j], remainder[j]); }
            quotient[i] = take;
        }
        ctx.assert_eq(remainder[N], zero, "uint remainder overflow");
        let quotient = std::array::from_fn(|i| Self::sum_bits(ctx, &quotient[i*32..(i+1)*32]));
        (Self::from_limbs(ctx, quotient), Self::from_limbs(ctx, std::array::from_fn(|i| remainder[i])))
    }
    pub fn bitand<C: DPNContext<F>>(self, ctx: &mut C, other: Self) -> Self {
        let a = Self::from_limbs(ctx, self.0);
        let b = Self::from_limbs(ctx, other.0);
        Self(std::array::from_fn(|i| ctx.op_u32_and(a.0[i], b.0[i])))
    }
    pub fn bitor<C: DPNContext<F>>(self, ctx: &mut C, other: Self) -> Self {
        let a = Self::from_limbs(ctx, self.0);
        let b = Self::from_limbs(ctx, other.0);
        Self(std::array::from_fn(|i| ctx.op_u32_or(a.0[i], b.0[i])))
    }
    pub fn bitxor<C: DPNContext<F>>(self, ctx: &mut C, other: Self) -> Self {
        let a = Self::from_limbs(ctx, self.0);
        let b = Self::from_limbs(ctx, other.0);
        Self(std::array::from_fn(|i| ctx.op_u32_xor(a.0[i], b.0[i])))
    }
    pub fn not<C: DPNContext<F>>(self, ctx: &mut C) -> Self {
        let max = Self::constant(ctx, Uint([u32::MAX; N]));
        self.bitxor(ctx, max)
    }
    fn shift<C: DPNContext<F>>(self, ctx: &mut C, distance: F, left: bool) -> Self {
        if N == 1 {
            // Native lane: U32Shift* already folds distance >= 32 to zero.
            let shifted = if left { ctx.op_u32_shl(self.0[0], distance) } else { ctx.op_u32_shr(self.0[0], distance) };
            return Self(std::array::from_fn(|_| shifted));
        }
        let value = Self::from_limbs(ctx, self.0);
        let distance = ctx.op_cast_u32(distance);
        let distance_bits = Self::split_bits(ctx, distance, 32);
        let zero = ctx.op_const_u32(0);
        let width = 32 * N;
        let stages = (width as u32).next_power_of_two().trailing_zeros() as usize;
        let mut bits: Vec<_> = value.0.iter().flat_map(|limb| Self::split_bits(ctx, *limb, 32)).collect();
        for stage in 0..stages {
            let step = 1usize << stage;
            bits = (0usize..width).map(|i| {
                let source = if left { i.checked_sub(step) } else { i.checked_add(step).filter(|j| *j < width) };
                ctx.op_select(distance_bits[stage], source.map(|j| bits[j]).unwrap_or(zero), bits[i])
            }).collect();
        }
        let oversized = ctx.op_bool_or_many(&distance_bits[stages..]);
        let limbs = std::array::from_fn(|i| {
            let limb = Self::sum_bits(ctx, &bits[i*32..(i+1)*32]);
            ctx.op_select(oversized, zero, limb)
        });
        Self::from_limbs(ctx, limbs)
    }
    pub fn shl<C: DPNContext<F>>(self, ctx: &mut C, distance: F) -> Self { self.shift(ctx, distance, true) }
    pub fn shr<C: DPNContext<F>>(self, ctx: &mut C, distance: F) -> Self { self.shift(ctx, distance, false) }
    /// Native u32 lane only: `**` on wider uints is rejected by sema and never
    /// reaches the VM.
    pub fn pow<C: DPNContext<F>>(self, ctx: &mut C, other: Self) -> Self {
        assert!(N == 1, "pow is native to the u32 lane");
        let powered = ctx.op_u32_exp(self.0[0], other.0[0]);
        Self(std::array::from_fn(|_| powered))
    }
}


/// Width-generic uint operations expanding into the existing scalar `DPNContext`
/// opcodes, vendored from psy-node `ops/uint.rs` so the interpreter does not
/// depend on a psy-node revision that ships them natively.
pub trait UintExt<F: ContextFelt> {
    fn op_uint_add<const N: usize>(&mut self, a: [F; N], b: [F; N]) -> [F; N];
    fn op_uint_sub<const N: usize>(&mut self, a: [F; N], b: [F; N]) -> [F; N];
    fn op_uint_mul<const N: usize>(&mut self, a: [F; N], b: [F; N]) -> [F; N];
    fn op_uint_div_rem<const N: usize>(&mut self, a: [F; N], b: [F; N]) -> ([F; N], [F; N]);
    fn op_uint_eq<const N: usize>(&mut self, a: [F; N], b: [F; N]) -> F;
    fn op_uint_lt<const N: usize>(&mut self, a: [F; N], b: [F; N]) -> F;
    fn op_uint_and<const N: usize>(&mut self, a: [F; N], b: [F; N]) -> [F; N];
    fn op_uint_or<const N: usize>(&mut self, a: [F; N], b: [F; N]) -> [F; N];
    fn op_uint_xor<const N: usize>(&mut self, a: [F; N], b: [F; N]) -> [F; N];
    fn op_uint_not<const N: usize>(&mut self, a: [F; N]) -> [F; N];
    fn op_uint_shl<const N: usize>(&mut self, a: [F; N], distance: F) -> [F; N];
    fn op_uint_shr<const N: usize>(&mut self, a: [F; N], distance: F) -> [F; N];
    fn op_uint_from_felt<const N: usize>(&mut self, value: F) -> [F; N];
    fn op_uint_from_limbs<const N: usize>(&mut self, limbs: [F; N]) -> [F; N];
    fn op_uint_to_felt<const N: usize>(&mut self, limbs: [F; N]) -> F;
    fn op_uint_to_u32<const N: usize>(&mut self, limbs: [F; N]) -> F;
}

impl<F: ContextFelt, C: DPNContext<F>> UintExt<F> for C {
    fn op_uint_add<const N: usize>(&mut self, a: [F; N], b: [F; N]) -> [F; N] { UintValue(a).add(self, UintValue(b)).0 }
    fn op_uint_sub<const N: usize>(&mut self, a: [F; N], b: [F; N]) -> [F; N] { UintValue(a).sub(self, UintValue(b)).0 }
    fn op_uint_mul<const N: usize>(&mut self, a: [F; N], b: [F; N]) -> [F; N] { UintValue(a).mul(self, UintValue(b)).0 }
    fn op_uint_div_rem<const N: usize>(&mut self, a: [F; N], b: [F; N]) -> ([F; N], [F; N]) {
        let (quotient, remainder) = UintValue(a).div_rem(self, UintValue(b));
        (quotient.0, remainder.0)
    }
    fn op_uint_eq<const N: usize>(&mut self, a: [F; N], b: [F; N]) -> F { UintValue(a).eq(self, UintValue(b)) }
    fn op_uint_lt<const N: usize>(&mut self, a: [F; N], b: [F; N]) -> F { UintValue(a).lt(self, UintValue(b)) }
    fn op_uint_and<const N: usize>(&mut self, a: [F; N], b: [F; N]) -> [F; N] { UintValue(a).bitand(self, UintValue(b)).0 }
    fn op_uint_or<const N: usize>(&mut self, a: [F; N], b: [F; N]) -> [F; N] { UintValue(a).bitor(self, UintValue(b)).0 }
    fn op_uint_xor<const N: usize>(&mut self, a: [F; N], b: [F; N]) -> [F; N] { UintValue(a).bitxor(self, UintValue(b)).0 }
    fn op_uint_not<const N: usize>(&mut self, a: [F; N]) -> [F; N] { UintValue(a).not(self).0 }
    fn op_uint_shl<const N: usize>(&mut self, a: [F; N], distance: F) -> [F; N] { UintValue(a).shl(self, distance).0 }
    fn op_uint_shr<const N: usize>(&mut self, a: [F; N], distance: F) -> [F; N] { UintValue(a).shr(self, distance).0 }
    fn op_uint_from_felt<const N: usize>(&mut self, value: F) -> [F; N] { UintValue::<F, N>::from_felt(self, value).0 }
    fn op_uint_from_limbs<const N: usize>(&mut self, limbs: [F; N]) -> [F; N] { UintValue::<F, N>::from_limbs(self, limbs).0 }
    fn op_uint_to_felt<const N: usize>(&mut self, limbs: [F; N]) -> F { UintValue(limbs).to_felt(self) }
    fn op_uint_to_u32<const N: usize>(&mut self, limbs: [F; N]) -> F { UintValue(limbs).to_u32(self) }
}
