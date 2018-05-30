//! Converting Cretonne IR to text.
//!
//! The `write` module provides the `write_function` function which converts an IR `Function` to an
//! equivalent textual form. This textual form can be read back by the `cretonne-reader` crate.

use ir::{DataFlowGraph, Ebb, Function, Inst, SigRef, Type, Value, ValueDef};
use isa::{RegInfo, TargetIsa};
use packed_option::ReservedValue;
use std::fmt::{self, Error, Result, Write};
use std::result;
use std::string::String;

/// Write `func` to `w` as equivalent text.
/// Use `isa` to emit ISA-dependent annotations.
pub fn write_function(w: &mut Write, func: &Function, isa: Option<&TargetIsa>) -> Result {
    let regs = isa.map(TargetIsa::register_info);
    let regs = regs.as_ref();

    write!(w, "function ")?;
    write_spec(w, func, regs)?;
    writeln!(w, " {{")?;
    let mut any = write_preamble(w, func, regs)?;
    for ebb in &func.layout {
        if any {
            writeln!(w)?;
        }
        write_ebb(w, func, isa, ebb)?;
        any = true;
    }
    writeln!(w, "}}")
}

//----------------------------------------------------------------------
//
// Function spec.

fn write_spec(w: &mut Write, func: &Function, regs: Option<&RegInfo>) -> Result {
    write!(w, "{}{}", func.name, func.signature.display(regs))
}

fn write_preamble(
    w: &mut Write,
    func: &Function,
    regs: Option<&RegInfo>,
) -> result::Result<bool, Error> {
    let mut any = false;

    for (ss, slot) in func.stack_slots.iter() {
        any = true;
        writeln!(w, "    {} = {}", ss, slot)?;
    }

    for (gv, gv_data) in func.global_vars.iter() {
        any = true;
        writeln!(w, "    {} = {}", gv, gv_data)?;
    }

    for (heap, heap_data) in func.heaps.iter() {
        any = true;
        writeln!(w, "    {} = {}", heap, heap_data)?;
    }

    // Write out all signatures before functions since function declarations can refer to
    // signatures.
    for (sig, sig_data) in func.dfg.signatures.iter() {
        any = true;
        writeln!(w, "    {} = {}", sig, sig_data.display(regs))?;
    }

    for (fnref, ext_func) in func.dfg.ext_funcs.iter() {
        any = true;
        if ext_func.signature != SigRef::reserved_value() {
            writeln!(w, "    {} = {}", fnref, ext_func)?;
        }
    }

    for (jt, jt_data) in func.jump_tables.iter() {
        any = true;
        writeln!(w, "    {} = {}", jt, jt_data)?;
    }

    if let Some(stack_limit) = func.stack_limit {
        any = true;
        writeln!(w, "    stack_limit = {}", stack_limit)?;
    }

    Ok(any)
}

//----------------------------------------------------------------------
//
// Basic blocks

pub fn write_arg(w: &mut Write, func: &Function, regs: Option<&RegInfo>, arg: Value) -> Result {
    write!(w, "{}: {}", arg, func.dfg.value_type(arg))?;
    let loc = func.locations[arg];
    if loc.is_assigned() {
        write!(w, " [{}]", loc.display(regs))?
    }

    Ok(())
}

pub fn write_ebb_header(
    w: &mut Write,
    func: &Function,
    isa: Option<&TargetIsa>,
    ebb: Ebb,
    indent: usize,
) -> Result {
    // Write out the basic block header, outdented:
    //
    //    ebb1:
    //    ebb1(v1: i32):
    //    ebb10(v4: f64, v5: b1):
    //

    // The `indent` is the instruction indentation. EBB headers are 4 spaces out from that.
    write!(w, "{1:0$}{2}", indent - 4, "", ebb)?;

    let regs = isa.map(TargetIsa::register_info);
    let regs = regs.as_ref();

    let mut args = func.dfg.ebb_params(ebb).iter().cloned();
    match args.next() {
        None => return writeln!(w, ":"),
        Some(arg) => {
            write!(w, "(")?;
            write_arg(w, func, regs, arg)?;
        }
    }
    // Remaining arguments.
    for arg in args {
        write!(w, ", ")?;
        write_arg(w, func, regs, arg)?;
    }
    writeln!(w, "):")
}

pub fn write_ebb(w: &mut Write, func: &Function, isa: Option<&TargetIsa>, ebb: Ebb) -> Result {
    // Indent all instructions if any encodings are present.
    let indent = if func.encodings.is_empty() && func.srclocs.is_empty() {
        4
    } else {
        36
    };

    write_ebb_header(w, func, isa, ebb, indent)?;
    for inst in func.layout.ebb_insts(ebb) {
        write_instruction(w, func, isa, inst, indent)?;
    }
    Ok(())
}

//----------------------------------------------------------------------
//
// Instructions

// Should `inst` be printed with a type suffix?
//
// Polymorphic instructions may need a suffix indicating the value of the controlling type variable
// if it can't be trivially inferred.
//
fn type_suffix(func: &Function, inst: Inst) -> Option<Type> {
    let inst_data = &func.dfg[inst];
    let constraints = inst_data.opcode().constraints();

    if !constraints.is_polymorphic() {
        return None;
    }

    // If the controlling type variable can be inferred from the type of the designated value input
    // operand, we don't need the type suffix.
    if constraints.use_typevar_operand() {
        let ctrl_var = inst_data.typevar_operand(&func.dfg.value_lists).unwrap();
        let def_ebb = match func.dfg.value_def(ctrl_var) {
            ValueDef::Result(instr, _) => func.layout.inst_ebb(instr),
            ValueDef::Param(ebb, _) => Some(ebb),
        };
        if def_ebb.is_some() && def_ebb == func.layout.inst_ebb(inst) {
            return None;
        }
    }

    let rtype = func.dfg.ctrl_typevar(inst);
    assert!(
        !rtype.is_void(),
        "Polymorphic instruction must produce a result"
    );
    Some(rtype)
}

// Write out any value aliases appearing in `inst`.
fn write_value_aliases(w: &mut Write, func: &Function, inst: Inst, indent: usize) -> Result {
    for &arg in func.dfg.inst_args(inst) {
        let resolved = func.dfg.resolve_aliases(arg);
        if resolved != arg {
            writeln!(w, "{1:0$}{2} -> {3}", indent, "", arg, resolved)?;
        }
    }
    Ok(())
}

fn write_instruction(
    w: &mut Write,
    func: &Function,
    isa: Option<&TargetIsa>,
    inst: Inst,
    indent: usize,
) -> Result {
    // Value aliases come out on lines before the instruction using them.
    write_value_aliases(w, func, inst, indent)?;

    // Prefix containing source location, encoding, and value locations.
    let mut s = String::with_capacity(16);

    // Source location goes first.
    let srcloc = func.srclocs[inst];
    if !srcloc.is_default() {
        write!(s, "{} ", srcloc)?;
    }

    // Write out encoding info.
    if let Some(enc) = func.encodings.get(inst).cloned() {
        if let Some(isa) = isa {
            write!(s, "[{}", isa.encoding_info().display(enc))?;
            // Write value locations, if we have them.
            if !func.locations.is_empty() {
                let regs = isa.register_info();
                for &r in func.dfg.inst_results(inst) {
                    write!(s, ",{}", func.locations[r].display(&regs))?
                }
            }
            write!(s, "] ")?;
        } else {
            write!(s, "[{}] ", enc)?;
        }
    }

    // Write out prefix and indent the instruction.
    write!(w, "{1:0$}", indent, s)?;

    // Write out the result values, if any.
    let mut has_results = false;
    for r in func.dfg.inst_results(inst) {
        if !has_results {
            has_results = true;
            write!(w, "{}", r)?;
        } else {
            write!(w, ", {}", r)?;
        }
    }
    if has_results {
        write!(w, " = ")?;
    }

    // Then the opcode, possibly with a '.type' suffix.
    let opcode = func.dfg[inst].opcode();

    match type_suffix(func, inst) {
        Some(suf) => write!(w, "{}.{}", opcode, suf)?,
        None => write!(w, "{}", opcode)?,
    }

    write_operands(w, &func.dfg, isa, inst)?;
    writeln!(w)
}

/// Write the operands of `inst` to `w` with a prepended space.
pub fn write_operands(
    w: &mut Write,
    dfg: &DataFlowGraph,
    isa: Option<&TargetIsa>,
    inst: Inst,
) -> Result {
    let pool = &dfg.value_lists;
    use ir::instructions::InstructionData::*;
    match dfg[inst] {
        Unary { arg, .. } => write!(w, " {}", arg),
        UnaryImm { imm, .. } => write!(w, " {}", imm),
        UnaryIeee32 { imm, .. } => write!(w, " {}", imm),
        UnaryIeee64 { imm, .. } => write!(w, " {}", imm),
        UnaryBool { imm, .. } => write!(w, " {}", imm),
        UnaryGlobalVar { global_var, .. } => write!(w, " {}", global_var),
        Binary { args, .. } => write!(w, " {}, {}", args[0], args[1]),
        BinaryImm { arg, imm, .. } => write!(w, " {}, {}", arg, imm),
        Ternary { args, .. } => write!(w, " {}, {}, {}", args[0], args[1], args[2]),
        MultiAry { ref args, .. } => {
            if args.is_empty() {
                write!(w, "")
            } else {
                write!(w, " {}", DisplayValues(args.as_slice(pool)))
            }
        }
        NullAry { .. } => write!(w, " "),
        InsertLane { lane, args, .. } => write!(w, " {}, {}, {}", args[0], lane, args[1]),
        ExtractLane { lane, arg, .. } => write!(w, " {}, {}", arg, lane),
        IntCompare { cond, args, .. } => write!(w, " {} {}, {}", cond, args[0], args[1]),
        IntCompareImm { cond, arg, imm, .. } => write!(w, " {} {}, {}", cond, arg, imm),
        IntCond { cond, arg, .. } => write!(w, " {} {}", cond, arg),
        FloatCompare { cond, args, .. } => write!(w, " {} {}, {}", cond, args[0], args[1]),
        FloatCond { cond, arg, .. } => write!(w, " {} {}", cond, arg),
        IntSelect { cond, args, .. } => {
            write!(w, " {} {}, {}, {}", cond, args[0], args[1], args[2])
        }
        Jump {
            destination,
            ref args,
            ..
        } => {
            write!(w, " {}", destination)?;
            write_ebb_args(w, args.as_slice(pool))
        }
        Branch {
            destination,
            ref args,
            ..
        } => {
            let args = args.as_slice(pool);
            write!(w, " {}, {}", args[0], destination)?;
            write_ebb_args(w, &args[1..])
        }
        BranchInt {
            cond,
            destination,
            ref args,
            ..
        } => {
            let args = args.as_slice(pool);
            write!(w, " {} {}, {}", cond, args[0], destination)?;
            write_ebb_args(w, &args[1..])
        }
        BranchFloat {
            cond,
            destination,
            ref args,
            ..
        } => {
            let args = args.as_slice(pool);
            write!(w, " {} {}, {}", cond, args[0], destination)?;
            write_ebb_args(w, &args[1..])
        }
        BranchIcmp {
            cond,
            destination,
            ref args,
            ..
        } => {
            let args = args.as_slice(pool);
            write!(w, " {} {}, {}, {}", cond, args[0], args[1], destination)?;
            write_ebb_args(w, &args[2..])
        }
        BranchTable { arg, table, .. } => write!(w, " {}, {}", arg, table),
        Call {
            func_ref, ref args, ..
        } => write!(w, " {}({})", func_ref, DisplayValues(args.as_slice(pool))),
        CallIndirect {
            sig_ref, ref args, ..
        } => {
            let args = args.as_slice(pool);
            write!(
                w,
                " {}, {}({})",
                sig_ref,
                args[0],
                DisplayValues(&args[1..])
            )
        }
        FuncAddr { func_ref, .. } => write!(w, " {}", func_ref),
        StackLoad {
            stack_slot, offset, ..
        } => write!(w, " {}{}", stack_slot, offset),
        StackStore {
            arg,
            stack_slot,
            offset,
            ..
        } => write!(w, " {}, {}{}", arg, stack_slot, offset),
        HeapAddr { heap, arg, imm, .. } => write!(w, " {}, {}, {}", heap, arg, imm),
        Load {
            flags, arg, offset, ..
        } => write!(w, "{} {}{}", flags, arg, offset),
        LoadComplex {
            flags,
            ref args,
            offset,
            ..
        } => {
            let args = args.as_slice(pool);
            write!(
                w,
                "{} {}{}",
                flags,
                DisplayValuesWithDelimiter(&args, '+'),
                offset
            )
        }
        Store {
            flags,
            args,
            offset,
            ..
        } => write!(w, "{} {}, {}{}", flags, args[0], args[1], offset),
        StoreComplex {
            flags,
            ref args,
            offset,
            ..
        } => {
            let args = args.as_slice(pool);
            write!(
                w,
                "{} {}, {}{}",
                flags,
                args[0],
                DisplayValuesWithDelimiter(&args[1..], '+'),
                offset
            )
        }
        RegMove { arg, src, dst, .. } => {
            if let Some(isa) = isa {
                let regs = isa.register_info();
                write!(
                    w,
                    " {}, {} -> {}",
                    arg,
                    regs.display_regunit(src),
                    regs.display_regunit(dst)
                )
            } else {
                write!(w, " {}, %{} -> %{}", arg, src, dst)
            }
        }
        CopySpecial { src, dst, .. } => {
            if let Some(isa) = isa {
                let regs = isa.register_info();
                write!(
                    w,
                    " {} -> {}",
                    regs.display_regunit(src),
                    regs.display_regunit(dst)
                )
            } else {
                write!(w, " %{} -> %{}", src, dst)
            }
        }
        RegSpill { arg, src, dst, .. } => {
            if let Some(isa) = isa {
                let regs = isa.register_info();
                write!(w, " {}, {} -> {}", arg, regs.display_regunit(src), dst)
            } else {
                write!(w, " {}, %{} -> {}", arg, src, dst)
            }
        }
        RegFill { arg, src, dst, .. } => {
            if let Some(isa) = isa {
                let regs = isa.register_info();
                write!(w, " {}, {} -> {}", arg, src, regs.display_regunit(dst))
            } else {
                write!(w, " {}, {} -> %{}", arg, src, dst)
            }
        }
        Trap { code, .. } => write!(w, " {}", code),
        CondTrap { arg, code, .. } => write!(w, " {}, {}", arg, code),
        IntCondTrap {
            cond, arg, code, ..
        } => write!(w, " {} {}, {}", cond, arg, code),
        FloatCondTrap {
            cond, arg, code, ..
        } => write!(w, " {} {}, {}", cond, arg, code),
    }
}

/// Write EBB args using optional parantheses.
fn write_ebb_args(w: &mut Write, args: &[Value]) -> Result {
    if args.is_empty() {
        Ok(())
    } else {
        write!(w, "({})", DisplayValues(args))
    }
}

/// Displayable slice of values.
struct DisplayValues<'a>(&'a [Value]);

impl<'a> fmt::Display for DisplayValues<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result {
        for (i, val) in self.0.iter().enumerate() {
            if i == 0 {
                write!(f, "{}", val)?;
            } else {
                write!(f, ", {}", val)?;
            }
        }
        Ok(())
    }
}

struct DisplayValuesWithDelimiter<'a>(&'a [Value], char);

impl<'a> fmt::Display for DisplayValuesWithDelimiter<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result {
        for (i, val) in self.0.iter().enumerate() {
            if i == 0 {
                write!(f, "{}", val)?;
            } else {
                write!(f, "{}{}", self.1, val)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use ir::types;
    use ir::{ExternalName, Function, StackSlotData, StackSlotKind};
    use std::string::ToString;

    #[test]
    fn basic() {
        let mut f = Function::new();
        assert_eq!(f.to_string(), "function u0:0() fast {\n}\n");

        f.name = ExternalName::testcase("foo");
        assert_eq!(f.to_string(), "function %foo() fast {\n}\n");

        f.create_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 4));
        assert_eq!(
            f.to_string(),
            "function %foo() fast {\n    ss0 = explicit_slot 4\n}\n"
        );

        let ebb = f.dfg.make_ebb();
        f.layout.append_ebb(ebb);
        assert_eq!(
            f.to_string(),
            "function %foo() fast {\n    ss0 = explicit_slot 4\n\nebb0:\n}\n"
        );

        f.dfg.append_ebb_param(ebb, types::I8);
        assert_eq!(
            f.to_string(),
            "function %foo() fast {\n    ss0 = explicit_slot 4\n\nebb0(v0: i8):\n}\n"
        );

        f.dfg.append_ebb_param(ebb, types::F32.by(4).unwrap());
        assert_eq!(
            f.to_string(),
            "function %foo() fast {\n    ss0 = explicit_slot 4\n\nebb0(v0: i8, v1: f32x4):\n}\n"
        );
    }
}
