//! Instruction formats and opcodes.
//!
//! The `instructions` module contains definitions for instruction formats, opcodes, and the
//! in-memory representation of IL instructions.
//!
//! A large part of this module is auto-generated from the instruction descriptions in the meta
//! directory.

use std::fmt::{self, Display, Formatter};
use std::str::FromStr;
use std::ops::{Deref, DerefMut};

use ir::{Value, Type, Ebb, JumpTable, SigRef, FuncRef};
use ir::immediates::{Imm64, Uimm8, Ieee32, Ieee64, ImmVector};
use ir::condcodes::*;
use ir::types;

use ref_slice::*;

// Include code generated by `lib/cretonne/meta/gen_instr.py`. This file contains:
//
// - The `pub enum InstructionFormat` enum with all the instruction formats.
// - The `pub enum Opcode` definition with all known opcodes,
// - The `const OPCODE_FORMAT: [InstructionFormat; N]` table.
// - The private `fn opcode_name(Opcode) -> &'static str` function, and
// - The hash table `const OPCODE_HASH_TABLE: [Opcode; N]`.
//
// For value type constraints:
//
// - The `const OPCODE_CONSTRAINTS : [OpcodeConstraints; N]` table.
// - The `const TYPE_SETS : [ValueTypeSet; N]` table.
// - The `const OPERAND_CONSTRAINTS : [OperandConstraint; N]` table.
//
include!(concat!(env!("OUT_DIR"), "/opcodes.rs"));

impl Display for Opcode {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}", opcode_name(*self))
    }
}

impl Opcode {
    /// Get the instruction format for this opcode.
    pub fn format(self) -> Option<InstructionFormat> {
        if self == Opcode::NotAnOpcode {
            None
        } else {
            Some(OPCODE_FORMAT[self as usize - 1])
        }
    }

    /// Get the constraint descriptor for this opcode.
    /// Panic if this is called on `NotAnOpcode`.
    pub fn constraints(self) -> OpcodeConstraints {
        OPCODE_CONSTRAINTS[self as usize - 1]
    }
}

// This trait really belongs in lib/reader where it is used by the .cton file parser, but since it
// critically depends on the `opcode_name()` function which is needed here anyway, it lives in this
// module. This also saves us from running the build script twice to generate code for the two
// separate crates.
impl FromStr for Opcode {
    type Err = &'static str;

    /// Parse an Opcode name from a string.
    fn from_str(s: &str) -> Result<Opcode, &'static str> {
        use constant_hash::{Table, simple_hash, probe};

        impl<'a> Table<&'a str> for [Opcode] {
            fn len(&self) -> usize {
                self.len()
            }

            fn key(&self, idx: usize) -> Option<&'a str> {
                if self[idx] == Opcode::NotAnOpcode {
                    None
                } else {
                    Some(opcode_name(self[idx]))
                }
            }
        }

        match probe::<&str, [Opcode]>(&OPCODE_HASH_TABLE, s, simple_hash(s)) {
            None => Err("Unknown opcode"),
            Some(i) => Ok(OPCODE_HASH_TABLE[i]),
        }
    }
}

/// Contents on an instruction.
///
/// Every variant must contain `opcode` and `ty` fields. An instruction that doesn't produce a
/// value should have its `ty` field set to `VOID`. The size of `InstructionData` should be kept at
/// 16 bytes on 64-bit architectures. If more space is needed to represent an instruction, use a
/// `Box<AuxData>` to store the additional information out of line.
#[derive(Clone, Debug)]
#[allow(missing_docs)]
pub enum InstructionData {
    Nullary { opcode: Opcode, ty: Type },
    Unary {
        opcode: Opcode,
        ty: Type,
        arg: Value,
    },
    UnaryImm {
        opcode: Opcode,
        ty: Type,
        imm: Imm64,
    },
    UnaryIeee32 {
        opcode: Opcode,
        ty: Type,
        imm: Ieee32,
    },
    UnaryIeee64 {
        opcode: Opcode,
        ty: Type,
        imm: Ieee64,
    },
    UnaryImmVector {
        opcode: Opcode,
        ty: Type,
        data: Box<UnaryImmVectorData>,
    },
    UnarySplit {
        opcode: Opcode,
        ty: Type,
        second_result: Value,
        arg: Value,
    },
    Binary {
        opcode: Opcode,
        ty: Type,
        args: [Value; 2],
    },
    BinaryImm {
        opcode: Opcode,
        ty: Type,
        arg: Value,
        imm: Imm64,
    },
    // Same as BinaryImm, but the immediate is the lhs operand.
    BinaryImmRev {
        opcode: Opcode,
        ty: Type,
        arg: Value,
        imm: Imm64,
    },
    BinaryOverflow {
        opcode: Opcode,
        ty: Type,
        second_result: Value,
        args: [Value; 2],
    },
    Ternary {
        opcode: Opcode,
        ty: Type,
        args: [Value; 3],
    },
    TernaryOverflow {
        opcode: Opcode,
        ty: Type,
        second_result: Value,
        data: Box<TernaryOverflowData>,
    },
    InsertLane {
        opcode: Opcode,
        ty: Type,
        lane: Uimm8,
        args: [Value; 2],
    },
    ExtractLane {
        opcode: Opcode,
        ty: Type,
        lane: Uimm8,
        arg: Value,
    },
    IntCompare {
        opcode: Opcode,
        ty: Type,
        cond: IntCC,
        args: [Value; 2],
    },
    FloatCompare {
        opcode: Opcode,
        ty: Type,
        cond: FloatCC,
        args: [Value; 2],
    },
    Jump {
        opcode: Opcode,
        ty: Type,
        data: Box<JumpData>,
    },
    Branch {
        opcode: Opcode,
        ty: Type,
        data: Box<BranchData>,
    },
    BranchTable {
        opcode: Opcode,
        ty: Type,
        arg: Value,
        table: JumpTable,
    },
    Call {
        opcode: Opcode,
        ty: Type,
        second_result: Value,
        data: Box<CallData>,
    },
    IndirectCall {
        opcode: Opcode,
        ty: Type,
        second_result: Value,
        data: Box<IndirectCallData>,
    },
    Return {
        opcode: Opcode,
        ty: Type,
        data: Box<ReturnData>,
    },
}

/// A variable list of `Value` operands used for function call arguments and passing arguments to
/// basic blocks.
#[derive(Clone, Debug)]
pub struct VariableArgs(Vec<Value>);

impl VariableArgs {
    /// Create an empty argument list.
    pub fn new() -> VariableArgs {
        VariableArgs(Vec::new())
    }

    /// Add an argument to the end.
    pub fn push(&mut self, v: Value) {
        self.0.push(v)
    }

    /// Check if the list is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

// Coerce VariableArgs into a &[Value] slice.
impl Deref for VariableArgs {
    type Target = [Value];

    fn deref<'a>(&'a self) -> &'a [Value] {
        &self.0
    }
}

impl DerefMut for VariableArgs {
    fn deref_mut<'a>(&'a mut self) -> &'a mut [Value] {
        &mut self.0
    }
}

impl Display for VariableArgs {
    fn fmt(&self, fmt: &mut Formatter) -> fmt::Result {
        for (i, val) in self.0.iter().enumerate() {
            if i == 0 {
                try!(write!(fmt, "{}", val));
            } else {
                try!(write!(fmt, ", {}", val));
            }
        }
        Ok(())
    }
}

impl Default for VariableArgs {
    fn default() -> VariableArgs {
        VariableArgs::new()
    }
}

/// Payload data for `vconst`.
#[derive(Clone, Debug)]
pub struct UnaryImmVectorData {
    /// Raw vector data.
    pub imm: ImmVector,
}

impl Display for UnaryImmVectorData {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        try!(write!(f, "#"));
        for b in &self.imm {
            try!(write!(f, "{:02x}", b));
        }
        Ok(())
    }
}

/// Payload data for ternary instructions with multiple results, such as `iadd_carry`.
#[derive(Clone, Debug)]
pub struct TernaryOverflowData {
    /// Value arguments.
    pub args: [Value; 3],
}

impl Display for TernaryOverflowData {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}, {}, {}", self.args[0], self.args[1], self.args[2])
    }
}

/// Payload data for jump instructions. These need to carry lists of EBB arguments that won't fit
/// in the allowed InstructionData size.
#[derive(Clone, Debug)]
pub struct JumpData {
    /// Jump destination EBB.
    pub destination: Ebb,
    /// Arguments passed to destination EBB.
    pub varargs: VariableArgs,
}

impl Display for JumpData {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        if self.varargs.is_empty() {
            write!(f, "{}", self.destination)
        } else {
            write!(f, "{}({})", self.destination, self.varargs)
        }
    }
}

/// Payload data for branch instructions. These need to carry lists of EBB arguments that won't fit
/// in the allowed InstructionData size.
#[derive(Clone, Debug)]
pub struct BranchData {
    /// Value argument controlling the branch.
    pub arg: Value,
    /// Branch destination EBB.
    pub destination: Ebb,
    /// Arguments passed to destination EBB.
    pub varargs: VariableArgs,
}

impl BranchData {
    /// Get references to the arguments.
    pub fn arguments(&self) -> [&[Value]; 2] {
        [ref_slice(&self.arg), &self.varargs]
    }

    /// Get mutable references to the arguments.
    pub fn arguments_mut(&mut self) -> [&mut [Value]; 2] {
        [ref_slice_mut(&mut self.arg), &mut self.varargs]
    }
}

impl Display for BranchData {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        try!(write!(f, "{}, {}", self.arg, self.destination));
        if !self.varargs.is_empty() {
            try!(write!(f, "({})", self.varargs));
        }
        Ok(())
    }
}

/// Payload of a call instruction.
#[derive(Clone, Debug)]
pub struct CallData {
    /// Callee function.
    pub func_ref: FuncRef,

    /// Dynamically sized array containing call argument values.
    pub varargs: VariableArgs,
}

/// Payload of an indirect call instruction.
#[derive(Clone, Debug)]
pub struct IndirectCallData {
    /// Callee function.
    pub arg: Value,

    /// Signature of the callee function.
    pub sig_ref: SigRef,

    /// Dynamically sized array containing call argument values.
    pub varargs: VariableArgs,
}

impl IndirectCallData {
    /// Get references to the arguments.
    pub fn arguments(&self) -> [&[Value]; 2] {
        [ref_slice(&self.arg), &self.varargs]
    }

    /// Get mutable references to the arguments.
    pub fn arguments_mut(&mut self) -> [&mut [Value]; 2] {
        [ref_slice_mut(&mut self.arg), &mut self.varargs]
    }
}

/// Payload of a return instruction.
#[derive(Clone, Debug)]
pub struct ReturnData {
    /// Dynamically sized array containing return values.
    pub varargs: VariableArgs,
}

/// Analyzing an instruction.
///
/// Avoid large matches on instruction formats by using the methods defined here to examine
/// instructions.
impl InstructionData {
    /// Execute a closure once for each argument to this instruction.
    /// See also the `arguments()` method.
    pub fn each_arg<F>(&self, mut func: F)
        where F: FnMut(Value)
    {
        for part in &self.arguments() {
            for &arg in part.iter() {
                func(arg);
            }
        }
    }

    /// Execute a closure with a mutable reference to each argument to this instruction.
    /// See also the `arguments_mut()` method.
    pub fn each_arg_mut<F>(&mut self, mut func: F)
        where F: FnMut(&mut Value)
    {
        for part in &mut self.arguments_mut() {
            for arg in part.iter_mut() {
                func(arg);
            }
        }
    }

    /// Return information about the destination of a branch or jump instruction.
    ///
    /// Any instruction that can transfer control to another EBB reveals its possible destinations
    /// here.
    pub fn analyze_branch<'a>(&'a self) -> BranchInfo<'a> {
        match self {
            &InstructionData::Jump { ref data, .. } => {
                BranchInfo::SingleDest(data.destination, &data.varargs)
            }
            &InstructionData::Branch { ref data, .. } => {
                BranchInfo::SingleDest(data.destination, &data.varargs)
            }
            &InstructionData::BranchTable { table, .. } => BranchInfo::Table(table),
            _ => BranchInfo::NotABranch,
        }
    }

    /// Return information about a call instruction.
    ///
    /// Any instruction that can call another function reveals its call signature here.
    pub fn analyze_call<'a>(&'a self) -> CallInfo<'a> {
        match self {
            &InstructionData::Call { ref data, .. } => {
                CallInfo::Direct(data.func_ref, &data.varargs)
            }
            &InstructionData::IndirectCall { ref data, .. } => {
                CallInfo::Indirect(data.sig_ref, &data.varargs)
            }
            _ => CallInfo::NotACall,
        }
    }
}

/// Information about branch and jump instructions.
pub enum BranchInfo<'a> {
    /// This is not a branch or jump instruction.
    /// This instruction will not transfer control to another EBB in the function, but it may still
    /// affect control flow by returning or trapping.
    NotABranch,

    /// This is a branch or jump to a single destination EBB, possibly taking value arguments.
    SingleDest(Ebb, &'a [Value]),

    /// This is a jump table branch which can have many destination EBBs.
    Table(JumpTable),
}

/// Information about call instructions.
pub enum CallInfo<'a> {
    /// This is not a call instruction.
    NotACall,

    /// This is a direct call to an external function declared in the preamble. See
    /// `DataFlowGraph.ext_funcs`.
    Direct(FuncRef, &'a [Value]),

    /// This is an indirect call with the specified signature. See `DataFlowGraph.signatures`.
    Indirect(SigRef, &'a [Value]),
}

/// Value type constraints for a given opcode.
///
/// The `InstructionFormat` determines the constraints on most operands, but `Value` operands and
/// results are not determined by the format. Every `Opcode` has an associated
/// `OpcodeConstraints` object that provides the missing details.
#[derive(Clone, Copy)]
pub struct OpcodeConstraints {
    /// Flags for this opcode encoded as a bit field:
    ///
    /// Bits 0-2:
    ///     Number of fixed result values. This does not include `variable_args` results as are
    ///     produced by call instructions.
    ///
    /// Bit 3:
    ///     This opcode is polymorphic and the controlling type variable can be inferred from the
    ///     designated input operand. This is the `typevar_operand` index given to the
    ///     `InstructionFormat` meta language object. When bit 0 is not set, the controlling type
    ///     variable must be the first output value instead.
    flags: u8,

    /// Permitted set of types for the controlling type variable as an index into `TYPE_SETS`.
    typeset_offset: u8,

    /// Offset into `OPERAND_CONSTRAINT` table of the descriptors for this opcode. The first
    /// `fixed_results()` entries describe the result constraints, then follows constraints for the
    /// fixed `Value` input operands. The number of `Value` inputs is determined by the instruction
    /// format.
    constraint_offset: u16,
}

impl OpcodeConstraints {
    /// Can the controlling type variable for this opcode be inferred from the designated value
    /// input operand?
    /// This also implies that this opcode is polymorphic.
    pub fn use_typevar_operand(self) -> bool {
        (self.flags & 0x8) != 0
    }

    /// Get the number of *fixed* result values produced by this opcode.
    /// This does not include `variable_args` produced by calls.
    pub fn fixed_results(self) -> usize {
        (self.flags & 0x7) as usize
    }

    /// Get the offset into `TYPE_SETS` for the controlling type variable.
    /// Returns `None` if the instruction is not polymorphic.
    fn typeset_offset(self) -> Option<usize> {
        let offset = self.typeset_offset as usize;
        if offset < TYPE_SETS.len() {
            Some(offset)
        } else {
            None
        }
    }

    /// Get the offset into OPERAND_CONSTRAINTS where the descriptors for this opcode begin.
    fn constraint_offset(self) -> usize {
        self.constraint_offset as usize
    }

    /// Get the value type of result number `n`, having resolved the controlling type variable to
    /// `ctrl_type`.
    pub fn result_type(self, n: usize, ctrl_type: Type) -> Type {
        assert!(n < self.fixed_results(), "Invalid result index");
        OPERAND_CONSTRAINTS[self.constraint_offset() + n]
            .resolve(ctrl_type)
            .expect("Result constraints can't be free")
    }

    /// Get the typeset of allowed types for the controlling type variable in a polymorphic
    /// instruction.
    pub fn ctrl_typeset(self) -> Option<ValueTypeSet> {
        self.typeset_offset().map(|offset| TYPE_SETS[offset])
    }

    /// Is this instruction polymorphic?
    pub fn is_polymorphic(self) -> bool {
        self.ctrl_typeset().is_some()
    }
}

/// A value type set describes the permitted set of types for a type variable.
#[derive(Clone, Copy)]
pub struct ValueTypeSet {
    min_lanes: u8,
    max_lanes: u8,
    min_int: u8,
    max_int: u8,
    min_float: u8,
    max_float: u8,
    min_bool: u8,
    max_bool: u8,
}

impl ValueTypeSet {
    /// Is `scalar` part of the base type set?
    ///
    /// Note that the base type set does not have to be included in the type set proper.
    fn is_base_type(&self, scalar: Type) -> bool {
        let l2b = scalar.log2_lane_bits();
        if scalar.is_int() {
            self.min_int <= l2b && l2b < self.max_int
        } else if scalar.is_float() {
            self.min_float <= l2b && l2b < self.max_float
        } else if scalar.is_bool() {
            self.min_bool <= l2b && l2b < self.max_bool
        } else {
            false
        }
    }

    /// Does `typ` belong to this set?
    pub fn contains(&self, typ: Type) -> bool {
        let l2l = typ.log2_lane_count();
        self.min_lanes <= l2l && l2l < self.max_lanes && self.is_base_type(typ.lane_type())
    }

    /// Get an example member of this type set.
    ///
    /// This is used for error messages to avoid suggesting invalid types.
    pub fn example(&self) -> Type {
        let t = if self.max_int > 5 {
            types::I32
        } else if self.max_float > 5 {
            types::F32
        } else if self.max_bool > 5 {
            types::B32
        } else {
            types::B1
        };
        t.by(1 << self.min_lanes).unwrap()
    }
}

/// Operand constraints. This describes the value type constraints on a single `Value` operand.
enum OperandConstraint {
    /// This operand has a concrete value type.
    Concrete(Type),

    /// This operand can vary freely within the given type set.
    /// The type set is identified by its index into the TYPE_SETS constant table.
    Free(u8),

    /// This operand is the same type as the controlling type variable.
    Same,

    /// This operand is `ctrlType.lane_type()`.
    LaneOf,

    /// This operand is `ctrlType.as_bool()`.
    AsBool,

    /// This operand is `ctrlType.half_width()`.
    HalfWidth,

    /// This operand is `ctrlType.double_width()`.
    DoubleWidth,
}

impl OperandConstraint {
    /// Resolve this operand constraint into a concrete value type, given the value of the
    /// controlling type variable.
    /// Returns `None` if this is a free operand which is independent of the controlling type
    /// variable.
    pub fn resolve(&self, ctrl_type: Type) -> Option<Type> {
        use self::OperandConstraint::*;
        match *self {
            Concrete(t) => Some(t),
            Free(_) => None,
            Same => Some(ctrl_type),
            LaneOf => Some(ctrl_type.lane_type()),
            AsBool => Some(ctrl_type.as_bool()),
            HalfWidth => Some(ctrl_type.half_width().expect("invalid type for half_width")),
            DoubleWidth => Some(ctrl_type.double_width().expect("invalid type for double_width")),
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opcodes() {
        let x = Opcode::Iadd;
        let mut y = Opcode::Isub;

        assert!(x != y);
        y = Opcode::Iadd;
        assert_eq!(x, y);
        assert_eq!(x.format(), Some(InstructionFormat::Binary));

        assert_eq!(format!("{:?}", Opcode::IaddImm), "IaddImm");
        assert_eq!(Opcode::IaddImm.to_string(), "iadd_imm");

        // Check the matcher.
        assert_eq!("iadd".parse::<Opcode>(), Ok(Opcode::Iadd));
        assert_eq!("iadd_imm".parse::<Opcode>(), Ok(Opcode::IaddImm));
        assert_eq!("iadd\0".parse::<Opcode>(), Err("Unknown opcode"));
        assert_eq!("".parse::<Opcode>(), Err("Unknown opcode"));
        assert_eq!("\0".parse::<Opcode>(), Err("Unknown opcode"));
    }

    #[test]
    fn instruction_data() {
        use std::mem;
        // The size of the InstructionData enum is important for performance. It should not exceed
        // 16 bytes. Use `Box<FooData>` out-of-line payloads for instruction formats that require
        // more space than that.
        // It would be fine with a data structure smaller than 16 bytes, but what are the odds of
        // that?
        assert_eq!(mem::size_of::<InstructionData>(), 16);
    }

    #[test]
    fn value_set() {
        use ir::types::*;

        let vts = ValueTypeSet {
            min_lanes: 0,
            max_lanes: 8,
            min_int: 3,
            max_int: 7,
            min_float: 0,
            max_float: 0,
            min_bool: 3,
            max_bool: 7,
        };
        assert!(vts.contains(I32));
        assert!(vts.contains(I64));
        assert!(vts.contains(I32X4));
        assert!(!vts.contains(F32));
        assert!(!vts.contains(B1));
        assert!(vts.contains(B8));
        assert!(vts.contains(B64));
        assert_eq!(vts.example().to_string(), "i32");

        let vts = ValueTypeSet {
            min_lanes: 0,
            max_lanes: 8,
            min_int: 0,
            max_int: 0,
            min_float: 5,
            max_float: 7,
            min_bool: 3,
            max_bool: 7,
        };
        assert_eq!(vts.example().to_string(), "f32");

        let vts = ValueTypeSet {
            min_lanes: 1,
            max_lanes: 8,
            min_int: 0,
            max_int: 0,
            min_float: 5,
            max_float: 7,
            min_bool: 3,
            max_bool: 7,
        };
        assert_eq!(vts.example().to_string(), "f32x2");

        let vts = ValueTypeSet {
            min_lanes: 2,
            max_lanes: 8,
            min_int: 0,
            max_int: 0,
            min_float: 0,
            max_float: 0,
            min_bool: 3,
            max_bool: 7,
        };
        assert!(!vts.contains(B32X2));
        assert!(vts.contains(B32X4));
        assert_eq!(vts.example().to_string(), "b32x4");

        let vts = ValueTypeSet {
            // TypeSet(lanes=(1, 256), ints=(8, 64))
            min_lanes: 0,
            max_lanes: 9,
            min_int: 3,
            max_int: 7,
            min_float: 0,
            max_float: 0,
            min_bool: 0,
            max_bool: 0,
        };
        assert!(vts.contains(I32));
        assert!(vts.contains(I32X4));
    }
}
