mod into_runtime_form;

pub use into_runtime_form::*;
use smallvec::SmallVec;
use tracing::warn;

use crate::format::scenario::{
    instruction_elements::{
        CodeAddress, FromNumber, NumberSpec, Register, RegisterRepr, UntypedNumberSpec,
    },
    instructions::{
        BinaryOperationType, Expression, ExpressionTerm, JumpCond, JumpCondType, UnaryOperationType,
    },
};

/// Contains the full VM state
///
/// It consists of a memory, two stacks (call and data)
#[derive(Clone)]
pub struct VmCtx {
    /// Memory (aka registers I guess)
    regular_registers: [i32; 0x1000],
    /// Call stack
    ///
    /// Stores the return address for each call instruction
    ///
    /// Native GOSUB/CALL return addresses and PUSH/POP values share this stack.
    call_stack: Vec<CodeAddress>,
    /// Data stack
    ///
    /// Stores the arguments for each call instruction (up to 255 encoded values).
    ///
    /// Can be addressed via [Register] with value > 0x1000
    ///
    /// Also called mem3 in ShinDataUtil
    arguments_stack: Vec<SmallVec<[i32; 6]>>,
    /// PRNG state, updated on each instruction executed
    prng_state: u32,
}

#[inline]
fn bool(v: i32) -> bool {
    v != 0
}

#[inline]
fn unbool(v: bool) -> i32 {
    if v { -1 } else { 0 }
}

#[inline]
fn real(v: i32) -> f32 {
    v as f32 * 0.001
}

#[inline]
fn unreal(v: f32) -> i32 {
    (v * 1000.0) as i32
}

#[inline]
fn angle(v: i32) -> f32 {
    real(v % 1000) * std::f32::consts::TAU
}

#[inline]
fn unangle(v: f32) -> i32 {
    (v * 1000.0 / std::f32::consts::TAU) as i32
}

// AArch64 SDIV returns zero on division by zero and wraps MIN / -1.
#[inline]
fn integer_divide(left: i32, right: i32) -> i32 {
    if right == 0 {
        0
    } else {
        left.wrapping_div(right)
    }
}

#[inline]
fn integer_modulo(left: i32, right: i32) -> i32 {
    left.wrapping_sub(integer_divide(left, right).wrapping_mul(right))
}

impl VmCtx {
    pub fn new(init_val: i32, random_seed: u32) -> Self {
        let mut memory = [0; 0x1000];
        memory[0] = init_val;

        Self {
            regular_registers: memory,
            call_stack: Vec::new(),
            arguments_stack: Vec::new(),
            prng_state: random_seed,
        }
    }

    pub(super) fn get_prng_state(&self) -> u32 {
        self.prng_state
    }

    /// Native replay checkpoints rewind the yielded instruction's PRNG advance,
    /// retaining all registers and both complete active stacks. Kept within the
    /// VM module: callers must validate the command boundary before using it.
    pub(super) fn clone_with_prng(&self, state: u32) -> Self {
        let mut copy = self.clone();
        copy.prng_state = state;
        copy
    }

    /// Test-only state identity for route exploration: registers, both stacks
    /// and PRNG. Two equal fingerprints continue identically for equal feedback.
    #[cfg(test)]
    pub(crate) fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.regular_registers.hash(&mut hasher);
        for address in &self.call_stack {
            address.0.hash(&mut hasher);
        }
        for frame in &self.arguments_stack {
            frame.as_slice().hash(&mut hasher);
        }
        self.prng_state.hash(&mut hasher);
        hasher.finish()
    }

    /// Native game-specific stack capacity checks, used before mutating a frame.
    pub(super) fn stack_sizes(&self) -> (usize, usize, usize) {
        (
            self.call_stack.len(),
            self.arguments_stack.len(),
            self.arguments_stack.iter().map(|f| f.len() + 1).sum(),
        )
    }

    /// Get the value from memory
    ///
    /// The address can be a stack offset (mem3) or main memory address (mem1)
    #[inline]
    pub fn read_register(&self, register: Register) -> i32 {
        match register.repr() {
            RegisterRepr::Argument(index) => {
                let frame = self
                    .arguments_stack
                    .last()
                    .expect("Attempt to read argument on empty stack");
                frame[index as usize]
            }
            RegisterRepr::Regular(index) => self.regular_registers[index as usize],
        }
    }

    /// Set a memory address to a value
    ///
    /// The address can be a stack offset (mem3) or main memory address (mem1)
    #[inline]
    pub fn write_register(&mut self, register: Register, val: i32) {
        match register.repr() {
            RegisterRepr::Argument(index) => {
                let frame = self
                    .arguments_stack
                    .last_mut()
                    .expect("Attempt to write argument on empty stack");
                frame[index as usize] = val;
            }
            RegisterRepr::Regular(index) => self.regular_registers[index as usize] = val,
        }
    }

    /// Read NumberSpec from memory (or return the constant value)
    #[inline]
    pub fn get_number<T: FromNumber>(&self, number: NumberSpec<T>) -> T {
        let value = match number.into_untyped() {
            UntypedNumberSpec::Constant(c) => c,
            UntypedNumberSpec::Register(addr) => self.read_register(addr),
        };
        T::from_number(value)
    }

    /// Evaluate a unary operation using the original engine's 32-bit semantics.
    ///
    /// Executed comparisons against both supplied NSOs: handler0xeba70/0xec1a0,
    /// jump table 0x173e5f. Opcodes 7/8 call asinf/acosf through the PLT;
    /// opcodes 10/11 call popcount/ctz at 0x119440/0x119480.
    pub fn evaluate_unary_operation(&self, ty: UnaryOperationType, source: i32) -> i32 {
        match ty {
            UnaryOperationType::Zero => 0,
            // The historical name is misleading: the Switch handler uses MVN W0, W0.
            UnaryOperationType::Not16 => !source,
            UnaryOperationType::Negate => source.wrapping_neg(),
            UnaryOperationType::Abs => source.wrapping_abs(),
            UnaryOperationType::Sin => unreal(angle(source).sin()),
            UnaryOperationType::Cos => unreal(angle(source).cos()),
            UnaryOperationType::Tan => unreal(angle(source).tan()),
            UnaryOperationType::ASin => unangle(real(source).asin()),
            UnaryOperationType::ACos => unangle(real(source).acos()),
            UnaryOperationType::ATan => unangle(real(source).atan()),
            UnaryOperationType::CountOnes => source.count_ones() as i32,
            UnaryOperationType::TrailingZeros => source.trailing_zeros() as i32,
        }
    }

    /// Evaluate jump condition in this context
    pub fn compute_jump_condition(&self, cond: JumpCond, left: i32, right: i32) -> bool {
        let result = match cond.condition {
            JumpCondType::Equal => left == right,
            JumpCondType::NotEqual => left != right,
            JumpCondType::GreaterOrEqual => left >= right,
            JumpCondType::Greater => left > right,
            JumpCondType::LessOrEqual => left <= right,
            JumpCondType::Less => left < right,
            JumpCondType::BitwiseAndNotZero => (left & right) != 0,
            JumpCondType::BitSet => (left & 1i32.wrapping_shl(right as u32)) != 0,
        };

        if cond.is_negated { !result } else { result }
    }

    pub fn push_code_stack(&mut self, addr: CodeAddress) {
        self.call_stack.push(addr);
    }

    pub fn pop_code_stack(&mut self) -> CodeAddress {
        self.call_stack.pop().unwrap()
    }

    pub fn push_data_stack_frame(&mut self, val: &[i32]) {
        self.arguments_stack.push(SmallVec::from_slice(val));
    }

    pub fn pop_data_stack_frame(&mut self) {
        self.arguments_stack.pop().unwrap();
    }

    /// Active stack state in the native oracle's serialized order.
    #[cfg(test)]
    pub(super) fn reference_stack_snapshot(&self) -> Vec<i64> {
        let mut words = vec![self.call_stack.len() as i64];
        words.extend(self.call_stack.iter().map(|v| i64::from(v.0)));
        words.push(
            256 - self
                .arguments_stack
                .iter()
                .map(|f| f.len() as i64 + 1)
                .sum::<i64>(),
        );
        for frame in self.arguments_stack.iter().rev() {
            words.push(frame.len() as i64);
            words.extend(frame.iter().map(|&v| i64::from(v)));
        }
        words
    }

    /// Evaluate an RPN expression with native wrapping integer/fixed-point semantics.
    /// Executed handler0xec050/0xec780 comparisons cover every term and depth32.
    pub fn evaluate_expression(&self, expr: &Expression) -> i32 {
        let mut stack = SmallVec::<[i32; 16]>::new();
        for term in expr.iter() {
            match term {
                &ExpressionTerm::Push(v) => stack.push(self.get_number(v)),
                ExpressionTerm::Add => {
                    let right = stack.pop().unwrap();
                    let left = stack.pop().unwrap();
                    stack.push(left.wrapping_add(right));
                }
                ExpressionTerm::Subtract => {
                    let right = stack.pop().unwrap();
                    let left = stack.pop().unwrap();
                    stack.push(left.wrapping_sub(right));
                }
                ExpressionTerm::Multiply => {
                    let right = stack.pop().unwrap();
                    let left = stack.pop().unwrap();
                    stack.push(left.wrapping_mul(right));
                }
                ExpressionTerm::Divide => {
                    let right = stack.pop().unwrap();
                    let left = stack.pop().unwrap();
                    stack.push(integer_divide(left, right));
                }
                ExpressionTerm::Modulo => {
                    let right = stack.pop().unwrap();
                    let left = stack.pop().unwrap();
                    stack.push(integer_modulo(left, right));
                }
                ExpressionTerm::ShiftLeft => {
                    let right = stack.pop().unwrap();
                    let left = stack.pop().unwrap();
                    stack.push(left.wrapping_shl(right as u32));
                }
                ExpressionTerm::ShiftRight => {
                    let right = stack.pop().unwrap();
                    let left = stack.pop().unwrap();
                    stack.push(left.wrapping_shr(right as u32));
                }
                ExpressionTerm::BitwiseAnd => {
                    let right = stack.pop().unwrap();
                    let left = stack.pop().unwrap();
                    stack.push(left & right);
                }
                ExpressionTerm::BitwiseOr => {
                    let right = stack.pop().unwrap();
                    let left = stack.pop().unwrap();
                    stack.push(left | right);
                }
                ExpressionTerm::BitwiseXor => {
                    let right = stack.pop().unwrap();
                    let left = stack.pop().unwrap();
                    stack.push(left ^ right);
                }
                ExpressionTerm::Negate => {
                    let val = stack.pop().unwrap();
                    stack.push(val.wrapping_neg());
                }
                ExpressionTerm::BitwiseNot => {
                    let val = stack.pop().unwrap();
                    stack.push(!val);
                }
                ExpressionTerm::Abs => {
                    let val = stack.pop().unwrap();
                    stack.push(val.wrapping_abs());
                }
                ExpressionTerm::CmpEqual => {
                    let right = stack.pop().unwrap();
                    let left = stack.pop().unwrap();
                    stack.push(unbool(left == right));
                }
                ExpressionTerm::CmpNotEqual => {
                    let right = stack.pop().unwrap();
                    let left = stack.pop().unwrap();
                    stack.push(unbool(left != right));
                }
                ExpressionTerm::CmpGreater => {
                    let right = stack.pop().unwrap();
                    let left = stack.pop().unwrap();
                    stack.push(unbool(left > right));
                }
                ExpressionTerm::CmpGreaterOrEqual => {
                    let right = stack.pop().unwrap();
                    let left = stack.pop().unwrap();
                    stack.push(unbool(left >= right));
                }
                ExpressionTerm::CmpLessOrEqual => {
                    let right = stack.pop().unwrap();
                    let left = stack.pop().unwrap();
                    stack.push(unbool(left <= right));
                }
                ExpressionTerm::CmpLess => {
                    let right = stack.pop().unwrap();
                    let left = stack.pop().unwrap();
                    stack.push(unbool(left < right));
                }
                ExpressionTerm::CmpZero => {
                    let val = stack.pop().unwrap();
                    stack.push(unbool(val == 0));
                }
                ExpressionTerm::CmpNotZero => {
                    let val = stack.pop().unwrap();
                    stack.push(unbool(val != 0));
                }
                ExpressionTerm::LogicalAnd => {
                    let right = stack.pop().unwrap();
                    let left = stack.pop().unwrap();
                    stack.push(unbool(bool(left) && bool(right)));
                }
                ExpressionTerm::LogicalOr => {
                    let right = stack.pop().unwrap();
                    let left = stack.pop().unwrap();
                    stack.push(unbool(bool(left) || bool(right)));
                }
                ExpressionTerm::Select => {
                    let cond = stack.pop().unwrap();
                    let true_val = stack.pop().unwrap();
                    let false_val = stack.pop().unwrap();
                    stack.push(if bool(cond) { true_val } else { false_val });
                }
                ExpressionTerm::MultiplyReal => {
                    let right = stack.pop().unwrap();
                    let left = stack.pop().unwrap();
                    stack.push(left.wrapping_mul(right) / 1000);
                }
                ExpressionTerm::DivideReal => {
                    let right = stack.pop().unwrap();
                    let left = stack.pop().unwrap();
                    stack.push(integer_divide(left.wrapping_mul(1000), right));
                }
                ExpressionTerm::Sin => {
                    let val = stack.pop().unwrap();
                    stack.push(unreal(angle(val).sin()));
                }
                ExpressionTerm::Cos => {
                    let val = stack.pop().unwrap();
                    stack.push(unreal(angle(val).cos()));
                }
                ExpressionTerm::Tan => {
                    let val = stack.pop().unwrap();
                    stack.push(unreal(angle(val).tan()));
                }
                ExpressionTerm::Min => {
                    let right = stack.pop().unwrap();
                    let left = stack.pop().unwrap();
                    stack.push(left.min(right));
                }
                ExpressionTerm::Max => {
                    let right = stack.pop().unwrap();
                    let left = stack.pop().unwrap();
                    stack.push(left.max(right));
                }
            }
        }
        if stack.len() != 1 {
            warn!("Expression did not evaluate to a single value");
        }

        stack.pop().unwrap()
    }

    /// The Switch handler at 0xebd50 uses 32-bit integer operations, including
    /// fixed point multiply/divide, rather than converting through floating point.
    /// Executed original0xebd50/updated0xec480 comparisons cover every operator.
    pub fn evaluate_binary_operation(&self, ty: BinaryOperationType, left: i32, right: i32) -> i32 {
        match ty {
            BinaryOperationType::MovRight => right,
            BinaryOperationType::Zero => 0,
            BinaryOperationType::Add => left.wrapping_add(right),
            BinaryOperationType::Subtract => left.wrapping_sub(right),
            BinaryOperationType::Multiply => left.wrapping_mul(right),
            BinaryOperationType::Divide => integer_divide(left, right),
            BinaryOperationType::Modulo => integer_modulo(left, right),
            BinaryOperationType::BitwiseAnd => left & right,
            BinaryOperationType::BitwiseOr => left | right,
            BinaryOperationType::BitwiseXor => left ^ right,
            BinaryOperationType::LeftShift => left.wrapping_shl(right as u32),
            BinaryOperationType::RightShift => left.wrapping_shr(right as u32),
            BinaryOperationType::MultiplyReal => left.wrapping_mul(right) / 1000,
            BinaryOperationType::DivideReal => integer_divide(left.wrapping_mul(1000), right),
            BinaryOperationType::ATan2 => unangle(f32::atan2(real(left), real(right))),
            BinaryOperationType::SetBit => {
                if (0..32).contains(&right) {
                    left | (1 << right)
                } else {
                    left
                }
            }
            BinaryOperationType::ClearBit => {
                if (0..32).contains(&right) {
                    left & !(1 << right)
                } else {
                    left
                }
            }
            BinaryOperationType::ACursedOperation => {
                let bits = left & (-1i32).wrapping_shl(right as u32);
                if bits == 0 {
                    -1
                } else {
                    bits.trailing_zeros() as i32
                }
            }
        }
    }

    /// Update the PRNG state.
    /// Native eed58/ef488 advances this immediately before each instruction.
    pub fn update_prng(&mut self) {
        self.prng_state = self.prng_state.wrapping_mul(0x343fd).wrapping_add(0x269ec3);
    }

    /// Generate a random number between the two endpoints, inclusive, in either order.
    ///
    /// This is called by the `rand` instruction
    ///
    /// The PRNG state is **NOT** updated after this call
    ///
    /// The caller advances it through [Self::update_prng] before the instruction.
    pub fn run_prng(&self, a: i32, b: i32) -> i32 {
        if a == b {
            a
        } else {
            // Native ecd04..ecd40/ed434..ed470: signed endpoints are
            // extended before range arithmetic; all24 upper seed bits remain.
            let lower = i64::from(a.min(b));
            let width = i64::from(a.max(b)) - lower + 1;
            let sample = i64::from(self.prng_state >> 8);
            (lower + ((width * sample) >> 24)) as i32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expression_binary(ctx: &VmCtx, term: ExpressionTerm, left: i32, right: i32) -> i32 {
        ctx.evaluate_expression(
            &Expression::new([
                ExpressionTerm::Push(NumberSpec::constant(left)),
                ExpressionTerm::Push(NumberSpec::constant(right)),
                term,
            ])
            .unwrap(),
        )
    }

    #[test]
    fn random_ranges_use_native_24bit_samples_and_wide_intervals() {
        // Results emitted by original ecc90 and updated ed3c0, without a VM step.
        for (seed, min, max, expected) in [
            (0x12345678, 0, 1000, 71),
            (0x80000000, i32::MIN, i32::MAX, 0),
            (u32::MAX, i32::MIN, i32::MAX, 2147483392),
            (0, i32::MIN, i32::MAX, i32::MIN),
            (u32::MAX, 42, 42, 42),
        ] {
            let ctx = VmCtx::new(0, seed);
            assert_eq!(ctx.run_prng(min, max), expected);
            assert_eq!(ctx.run_prng(max, min), expected);
            assert_eq!(ctx.get_prng_state(), seed, "RND does not advance the seed");
        }
    }

    #[test]
    fn unary_integer_boundaries() {
        let ctx = VmCtx::new(0, 0);
        for (ty, input, expected) in [
            (UnaryOperationType::Zero, i32::MAX, 0),
            (UnaryOperationType::Not16, 0, -1),
            (UnaryOperationType::Not16, 0x12345678, !0x12345678),
            (UnaryOperationType::Negate, i32::MIN, i32::MIN),
            (UnaryOperationType::Abs, -123, 123),
            (UnaryOperationType::Abs, i32::MIN, i32::MIN),
            (UnaryOperationType::CountOnes, -1, 32),
            (UnaryOperationType::CountOnes, i32::MIN, 1),
            (UnaryOperationType::TrailingZeros, 0, 32),
            (UnaryOperationType::TrailingZeros, i32::MIN, 31),
            (UnaryOperationType::Sin, 250, 1000),
            (UnaryOperationType::Sin, -250, -1000),
            (UnaryOperationType::Sin, 1250, 1000),
            (UnaryOperationType::Cos, 0, 1000),
            (UnaryOperationType::Tan, 0, 0),
            // Single precision multiply/divide precedes truncation in FCVTZS.
            (UnaryOperationType::ASin, 1000, 249),
            (UnaryOperationType::ACos, 1000, 0),
            (UnaryOperationType::ATan, 0, 0),
        ] {
            assert_eq!(ctx.evaluate_unary_operation(ty, input), expected);
        }
    }

    #[test]
    fn binary_and_expression_integer_semantics_agree() {
        use BinaryOperationType as B;
        use ExpressionTerm as E;
        let ctx = VmCtx::new(0, 0);
        for (binary, term, left, right, expected) in [
            (B::Add, E::Add, i32::MAX, 1, i32::MIN),
            (B::Subtract, E::Subtract, i32::MIN, 1, i32::MAX),
            (B::Multiply, E::Multiply, i32::MAX, 2, -2),
            (B::Divide, E::Divide, i32::MIN, -1, i32::MIN),
            (B::Divide, E::Divide, 7, 0, 0),
            (B::Modulo, E::Modulo, -7, 3, -1),
            (B::Modulo, E::Modulo, 7, 0, 7),
            (B::Modulo, E::Modulo, i32::MIN, -1, 0),
            (B::LeftShift, E::ShiftLeft, 1, -1, i32::MIN),
            (B::LeftShift, E::ShiftLeft, 1, 32, 1),
            (B::RightShift, E::ShiftRight, i32::MIN, 63, -1),
            (B::MultiplyReal, E::MultiplyReal, -1500, 333, -499),
            (B::MultiplyReal, E::MultiplyReal, i32::MAX, 2, 0),
            (B::DivideReal, E::DivideReal, -1500, 3000, -500),
            (B::DivideReal, E::DivideReal, 1000, 0, 0),
        ] {
            assert_eq!(
                ctx.evaluate_binary_operation(binary, left, right),
                expected,
                "{binary:?}"
            );
            assert_eq!(
                expression_binary(&ctx, term, left, right),
                expected,
                "{term:?}"
            );
        }
    }

    #[test]
    fn bit_operations_follow_switch_boundaries() {
        let ctx = VmCtx::new(0, 0);
        for index in [-1, 32, 63] {
            assert_eq!(
                ctx.evaluate_binary_operation(BinaryOperationType::SetBit, 42, index),
                42
            );
            assert_eq!(
                ctx.evaluate_binary_operation(BinaryOperationType::ClearBit, 42, index),
                42
            );
        }
        assert_eq!(
            ctx.evaluate_binary_operation(BinaryOperationType::SetBit, 0, 31),
            i32::MIN
        );
        assert_eq!(
            ctx.evaluate_binary_operation(BinaryOperationType::ClearBit, -1, 31),
            i32::MAX
        );
        for (value, start, expected) in [(0, 0, -1), (8, 4, -1), (8, 0, 3), (i32::MIN, -1, 31)] {
            assert_eq!(
                ctx.evaluate_binary_operation(BinaryOperationType::ACursedOperation, value, start),
                expected
            );
        }
    }
}
