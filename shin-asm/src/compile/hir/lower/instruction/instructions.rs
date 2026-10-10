use shin_core::format::scenario::{
    instruction_elements::{CodeAddress, NumberSpec, Register},
    instructions::{Instruction, UnaryOperation, UnaryOperationType},
};

use crate::compile::hir::lower::instruction::router::{Router, RouterBuilder};

fn zero((destination, source): (Register, Option<NumberSpec>)) -> Instruction {
    Instruction::uo(UnaryOperation {
        ty: UnaryOperationType::Zero,
        destination,
        source: source.unwrap_or(NumberSpec::constant(0)),
    })
}

fn unary_op(instr_name: &str, (destination, source): (Register, NumberSpec)) -> Instruction {
    let ty = match instr_name {
        "not16" => UnaryOperationType::Not16,
        "neg" => UnaryOperationType::Negate,
        "abs" => UnaryOperationType::Abs,
        "sin" => UnaryOperationType::Sin,
        "cos" => UnaryOperationType::Cos,
        "tan" => UnaryOperationType::Tan,
        "asin" => UnaryOperationType::ASin,
        "acos" => UnaryOperationType::ACos,
        "atan" => UnaryOperationType::ATan,
        "popcnt" => UnaryOperationType::CountOnes,
        "ctz" => UnaryOperationType::TrailingZeros,
        _ => unreachable!(),
    };

    Instruction::uo(UnaryOperation {
        ty,
        destination,
        source,
    })
}

fn jump((target,): (CodeAddress,)) -> Instruction {
    Instruction::j { target }
}

pub fn instructions(builder: RouterBuilder<impl Router>) -> RouterBuilder<impl Router> {
    builder
        .add("zero", zero)
        .add("not16", unary_op)
        .add("neg", unary_op)
        .add("abs", unary_op)
        .add("sin", unary_op)
        .add("cos", unary_op)
        .add("tan", unary_op)
        .add("asin", unary_op)
        .add("acos", unary_op)
        .add("atan", unary_op)
        .add("popcnt", unary_op)
        .add("ctz", unary_op)
        .add("j", jump)
}
