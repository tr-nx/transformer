use crate::aarch64_reader::ExtendType::{SXTB, SXTH, SXTW, UXTB, UXTH};
use crate::aarch64_reader::FlagMasks::{SET_FLAGS, W32};
use crate::aarch64_reader::Op::{A64_ADD_IMM, A64_ADR, A64_ADRP, A64_AND_IMM, A64_ASR_IMM, A64_BFC, A64_BFI, A64_BFM, A64_BFXIL, A64_CMN_IMM, A64_CMP_IMM, A64_EOR_IMM, A64_EXTEND, A64_EXTR, A64_LSL_IMM, A64_LSR_IMM, A64_MOV_IMM, A64_MOV_SP, A64_MOVK, A64_ORR_IMM, A64_ROR_IMM, A64_SBFIZ, A64_SBFM, A64_SBFX, A64_SUB_IMM, A64_TST_IMM, A64_UBFIZ, A64_UBFM, A64_UBFX, A64_UNKNOWN};
use crate::aarch64_reader::OpKind::{AddSub, AddSubTags, Bitfield, Extract, Logic, Move, PCRelAddr, Unknown};
use crate::aarch64_reader::Registries::{STACK_POINTER, ZERO_REG};

///Register 31's interpretation is up to the instruction. Many interpret it as the
///zero register ZR/WZR. Reading to it yields a zero, writing discards the result.
///Other instructions interpret it as the stack pointer SP.
///
///We split up this overloaded register: when we encounter R31 and interpret it as
///the stack pointer, we assign a different number. This way, the user does not
///need to know which instructions use the SP and which use the ZR.
mod Registries {
    pub const ZERO_REG: u8 = 31;
    pub const STACK_POINTER: u8 = 100;
}

/// Opcodes ordered and grouped according to the Top-level Encodings
/// of the A64 Instruction Set Architecture (ARMv8-A profile) document,
/// pages 1406-1473.
///
/// Immediate and register variants generally have different opcodes
/// (e.g. A64_ADD_IMM, A64_ADD_SHIFTED, A64_ADD_EXT), but the marker
/// only appears where disambiguation is needed (e.g. ADR is not called
/// ADR_IMM since there is no register variant). Aliases have an opcode
/// of their own.
///
/// Where possible, variants of instructions with regular structure
/// are encoded as one instruction. For example, conditional branches
/// like B.EQ, B.PL and so on are encoded as A64_BCOND with the
/// condition encoded in the Inst.flags field. The various addressing
/// modes of loads and stores are encoded similarly. See the Inst
/// structure for more detail.
#[derive(Clone, PartialEq, Eq)]
pub enum Op {
    A64_UNKNOWN,
    /// unknown instruction (or Op field not set, by accident), Inst.imm contains raw binary instruction
    A64_ERROR,
    /// invalid instruction, Inst.error contains error string
    A64_UDF,
    /// throws undefined exception

    /*** Data Processing -- Immediate ***/

    /// PC-rel. addressing
    A64_ADR,
    /// ADR Xd, label  -- Xd ← PC + label
    A64_ADRP,
    /// ADRP Xd, label -- Xd ← PC + (label * 4K)

    /// Add/subtract (immediate, with tags) -- OMITTED

    /// Add/subtract (immediate)
    A64_ADD_IMM,
    A64_CMN_IMM,
    A64_MOV_SP,
    /// MOV from/to SP -- ADD (imm) alias (predicate: shift == 0 && imm12 == 0 && (Rd == SP || Rn == SP))
    A64_SUB_IMM,
    A64_CMP_IMM,

    /// Logical (immediate)
    A64_AND_IMM,
    A64_ORR_IMM,
    A64_EOR_IMM,
    A64_TST_IMM,
    /// TST Rn -- ANDS alias (Rd := RZR, predicate: Rd == ZR && set_flags)

    /// Move wide (immediate)
    A64_MOVK,
    /// keep other bits

    /// Synthetic instruction comprising MOV (bitmask immediate), MOV (inverted wide immediate)
    /// and MOV (wide immediate), MOVN and MOVZ; essentially all MOVs where the result of the
    /// operation can be precalculated. For lifting, we do not care how the immediate was encoded,
    /// only that it is an immediate move.
    A64_MOV_IMM,

    /// Bitfield
    A64_SBFM,
    /// always decoded to an alias
    A64_ASR_IMM,
    A64_SBFIZ,
    A64_SBFX,
    A64_BFM,
    /// always decoded to an alias
    A64_BFC,
    A64_BFI,
    A64_BFXIL,
    A64_UBFM,
    /// always decoded to an alias
    A64_LSL_IMM,
    A64_LSR_IMM,
    A64_UBFIZ,
    A64_UBFX,

    /// Synthetic instruction comprising the SXTB, SXTH, SXTW, UXTB and UXTH aliases of SBFM and UBFM.
    /// The kind of extension is stored in Inst.extend.type.
    A64_EXTEND,

    /// Extract
    A64_EXTR,
    A64_ROR_IMM,
    /// ROR Rd, Rs, #shift -- EXTR alias (Rm := Rs, Rn := Rs, predicate: Rm == Rn)

    /*** Branches, Exception Generating and System Instructions ***/

    A64_BCOND,

    /// Exception generation
    ///
    /// With the exception of SVC, they are not interesting for lifting
    /// userspace programs, but were included since they are trivial.
    A64_SVC,
    /// system call
    A64_HVC,
    A64_SMC,
    A64_BRK,
    A64_HLT,
    A64_DCPS1,
    A64_DCPS2,
    A64_DCPS3,

    /// Hints -- we treat all allocated hints as NOP and don't decode to the "aliases"
    /// NOP, YIELD, ...
    A64_HINT,

    /// Barriers
    A64_CLREX,
    A64_DMB,
    A64_ISB,
    A64_SB,
    A64_DSB,
    A64_SSBB,
    A64_PSSBB,

    /// PSTATE
    A64_MSR_IMM,
    /// MSR <pstatefield>, #imm -- Inst.msr_imm
    A64_CFINV,
    A64_XAFlag,
    /// irrelevant
    A64_AXFlag,
    /// ------

    /// System instructions -- Inst.ldst.rt := Xt
    A64_SYS,
    /// SYS #op1, Cn, Cm, #op2(, Xt)
    A64_SYSL,
    /// SYSL Xt, #op1, Cn, Cm, #op2

    /// System register move -- Inst.ldst.rt := Xt; Inst.imm := sysreg
    A64_MSR_REG,
    /// MSR <sysreg>, Xt
    A64_MRS,
    /// MRS Xt, <sysreg>

    /// Unconditional branch (register)
    A64_BR,
    A64_BLR,
    A64_RET,

    /// Unconditional branch (immediate)
    A64_B,
    A64_BL,

    /// Compare and branch (immediate)
    A64_CBZ,
    A64_CBNZ,

    /// Test and branch (immediate) -- Inst.tbz
    A64_TBZ,
    A64_TBNZ,

    /*** Data Processing -- Register ***/

    /// Data-processing (2 source)
    A64_UDIV,
    A64_SDIV,
    A64_LSLV,
    A64_LSRV,
    A64_ASRV,
    A64_RORV,
    A64_CRC32B,
    A64_CRC32H,
    A64_CRC32W,
    A64_CRC32X,
    A64_CRC32CB,
    A64_CRC32CH,
    A64_CRC32CW,
    A64_CRC32CX,
    A64_SUBP,

    /// Data-processing (1 source)
    A64_RBIT,
    A64_REV16,
    A64_REV,
    A64_REV32,
    A64_CLZ,
    A64_CLS,

    /// Logical (shifted register)
    A64_AND_SHIFTED,
    A64_TST_SHIFTED,
    /// ANDS alias (Rd := ZR, predicate: Rd == ZR)
    A64_BIC,
    A64_ORR_SHIFTED,
    A64_MOV_REG,
    /// ORR alias (predicate: shift == 0 && imm6 == 0 && Rn == ZR)
    A64_ORN,
    A64_MVN,
    /// ORN alias (Rn := ZR, predicate: Rn == ZR)
    A64_EOR_SHIFTED,
    A64_EON,

    /// Add/subtract (shifted register)
    A64_ADD_SHIFTED,
    A64_CMN_SHIFTED,
    /// ADDS alias (Rd := ZR, predicate: Rd == ZR && set_flags)
    A64_SUB_SHIFTED,
    A64_NEG,
    /// SUB alias (Rn := ZR, predicate: Rn == ZR)
    A64_CMP_SHIFTED,
    /// SUBS alias (Rd := ZR, predicate: Rd == ZR && set_flags)

    /// Add/subtract (extended register)
    /// Register 31 is interpreted as the stack pointer (SP/WSP).
    A64_ADD_EXT,
    A64_CMN_EXT,
    /// ADDS alias (Rd := ZR, predicate: Rd == ZR && set_flags)
    A64_SUB_EXT,
    A64_CMP_EXT,
    /// SUBS alias (Rd := ZR, predicate: Rd == ZR && set_flags)

    /// Add/subtract (with carry)
    A64_ADC,
    A64_SBC,
    A64_NGC,
    /// SBC alias (Rd := ZR, predicate: Rd == RR)

    /// Rotate right into flags
    A64_RMIF,

    /// Evaluate into flags
    A64_SETF8,
    A64_SETF16,

    /// Conditional compare (register)
    A64_CCMN_REG,
    A64_CCMP_REG,

    /// Conditional compare (immediate)
    A64_CCMN_IMM,
    A64_CCMP_IMM,

    /// Conditional select
    A64_CSEL,
    A64_CSINC,
    A64_CINC,
    /// CSINC alias (cond := invert(cond), predicate: Rm == Rn != ZR)
    A64_CSET,
    /// CSINC alias (cond := invert(cond), predicate: Rm == Rn == ZR)
    A64_CSINV,
    A64_CINV,
    /// CSINV alias (cond := invert(cond), predicate: Rm == Rn != ZR)
    A64_CSETM,
    /// CSINV alias (cond := invert(cond), predicate: Rm == Rn == ZR)
    A64_CSNEG,
    A64_CNEG,
    /// CSNEG alias (cond := invert(cond), predicate: Rm == Rn)

    /// Data-processing (3 source)
    A64_MADD,
    A64_MUL,
    /// MADD alias (Ra omitted, predicate: Ra == ZR)
    A64_MSUB,
    A64_MNEG,
    /// MSUB alias (^---- see above)
    A64_SMADDL,
    A64_SMULL,
    /// SMADDL alias  (^---- see above)
    A64_SMSUBL,
    A64_SMNEGL,
    /// SMSUBL alias (^---- see above)
    A64_SMULH,
    A64_UMADDL,
    A64_UMULL,
    /// UMADDL alias (^---- see above)
    A64_UMSUBL,
    A64_UMNEGL,
    /// UMSUBL alias (^---- see above)
    A64_UMULH,

    /*** Loads and Stores ***/

    /// There are not that many opcodes because access size, sign-extension
    /// and addressing mode (post-indexed, register offset, immediate) are
    /// encoded in the Inst, to leverage the regular structure and cut down
    /// on opcodes (and by extension, duplicative switch-cases for the user
    /// of this decoder).

    /// Advanced SIMD load/store multiple structures
    /// Advanced SIMD load/store multiple structures (post-indexed)
    A64_LD1_MULT,
    A64_ST1_MULT,
    A64_LD2_MULT,
    A64_ST2_MULT,
    A64_LD3_MULT,
    A64_ST3_MULT,
    A64_LD4_MULT,
    A64_ST4_MULT,

    /// Advanced SIMD load/store single structure
    /// Advanced SIMD load/store single structure (post-indexed)
    A64_LD1_SINGLE,
    A64_ST1_SINGLE,
    A64_LD2_SINGLE,
    A64_ST2_SINGLE,
    A64_LD3_SINGLE,
    A64_ST3_SINGLE,
    A64_LD4_SINGLE,
    A64_ST4_SINGLE,
    A64_LD1R,
    A64_LD2R,
    A64_LD3R,
    A64_LD4R,

    /// Load/store exclusive
    A64_LDXR,
    /// includes Load-acquire variants
    A64_STXR,
    /// includes Store-acquire variants (STLXR)
    A64_LDXP,
    /// ------
    A64_STXP,
    /// ------
    A64_LDAPR,
    /// Load-AcquirePC Register (actually in Atomic group)

    /// Load/store no-allocate pair (offset)
    A64_LDNP,
    A64_STNP,
    A64_LDNP_FP,
    A64_STNP_FP,

    /// Load-acquire/store-release register     -- AM_SIMPLE
    /// Load/store register pair (post-indexed) -- AM_POST
    /// Load/store register pair (offset)       -- AM_OFF_IMM
    /// Load/store register pair (pre-indexed)  -- AM_PRE
    A64_LDP,
    /// LDP, LDXP
    A64_STP,
    /// STP, STXP
    A64_LDP_FP,
    A64_STP_FP,

    /// Load/store register (unprivileged): unsupported system instructions

    /// Load register (literal)                      -- AM_LITERAL
    /// Load-acquire/store-release register          -- AM_SIMPLE
    /// Load-LOAcquire/Store-LORelease register      -- AM_SIMPLE
    /// Load/store register (immediate post-indexed) -- AM_POST
    /// Load/store register (immediate pre-indexed)  -- AM_PRE
    /// Load/store register (register offset)        -- AM_OFF_REG, AM_OFF_EXT
    /// Load/store register (unsigned immediate)     -- AM_OFF_IMM
    /// Load/store register (unscaled immediate)     -- AM_OFF_IMM
    A64_LDR,
    /// LDR, LDAR, LDLAR, LDUR
    A64_STR,
    /// STR, STLR, STLLR, STUR
    A64_LDR_FP,
    A64_STR_FP,

    /// Prefetch memory
    ///
    /// The exact prefetch operation is stored in Inst.rt := Rt.
    /// We cannot use a "struct prfm" because the addressing mode-specific
    /// data (offset, .extend) already occupies the space.
    ///
    /// PRFM (literal)          -- AM_LITERAL
    /// PRFM (register)         -- AM_OFF_EXT
    /// PRFM (immediate)        -- AM_OFF_IMM
    /// PRFUM (unscaled offset) -- AM_OFF_IMM
    A64_PRFM,

    /// Atomic memory operations
    ///
    /// Whether the instruction has load-acquire (e.g. LDADDA*), load-acquire/
    /// store-release (e.g. LDADDAL*) or store-release (e.g. STADDL) semantics
    /// is stored in ldst_order.load and .store.
    ///
    /// There are no ST* aliases; the only difference to the LD* instructions
    /// is that the original value of the memory cell is discarded by writing
    /// to the zero register.
    A64_LDADD,
    A64_LDCLR,
    A64_LDEOR,
    A64_LDSET,
    A64_LDSMAX,
    A64_LDSMIN,
    A64_LDUMAX,
    A64_LDUMIN,
    A64_SWP,
    A64_CAS,
    /// Compare and Swap (actually from Exclusive group)
    A64_CASP,
    /// Compare and Swap Pair of (double)words (actually from Exclusive group)

    /*** Data Processing -- Scalar Floating-Point and Advanced SIMD ***/

    /// The instructions are ordered by functionality here, because the order of the
    /// top-level encodings, as used in the other categories, splits variants of the
    /// same instruction. We want as few opcodes as possible.

    /// Conversion between Floating Point and Integer/Fixed-Point
    ///
    /// Sca: SIMD&FP register interpreted as a scalar (Hn, Sn, Dn).
    /// Vec: SIMD&FP register interpreted as a vector (Vn.<T>).
    /// GPR: General Purpose Register (Wn, Xn).
    ///
    /// Inst.flags.W32  := GPR bits == 32
    /// Inst.flags.prec := Sca(fp) precision (FPSize)
    /// Inst.flags.ext  := Vec(fp) vector arrangement
    /// Inst.fcvt.mode  := rounding mode
    /// Inst.fcvt.fbits := #fbits for fixed-point
    /// Inst.fcvt.typ   := signed OR unsigned OR fixed-point
    A64_FCVT_GPR,
    /// Sca(fp)        → GPR(int|fixed)
    A64_FCVT_VEC,
    /// Vec(fp)        → Vec(int|fixed)
    A64_CVTF,
    /// GPR(int|fixed) → Sca(fp)
    A64_CVTF_VEC,
    /// Vec(int|fixed) → Vec(fp)
    A64_FJCVTZS,
    /// Sca(f32)       → GPR(i32); special Javascript instruction

    /// Rounding and Precision Conversion
    ///
    /// Inst.flags.prec := Sca(fp) precision
    /// Inst.frint.mode := rounding mode
    /// Inst.frint.bits := 0 if any size, 32, 64
    A64_FRINT,
    /// Round to integral (any size, 32-bit, or 64-bit)
    A64_FRINT_VEC,
    A64_FRINTX,
    /// ---- Exact (throws Inexact exception on failure)
    A64_FRINTX_VEC,
    A64_FCVT_H,
    /// Convert from any precision to Half
    A64_FCVT_S,
    /// -------------------------- to Single
    A64_FCVT_D,
    /// -------------------------- to Double
    A64_FCVTL,
    /// Extend to higher precision (vector)
    A64_FCVTN,
    /// Narrow to lower precision  (vector)
    A64_FCVTXN,
    /// Narrow to lower precision, round to odd (vector)

    /// Floating-Point Computation (scalar)
    A64_FABS,
    A64_FNEG,
    A64_FSQRT,
    A64_FMUL,
    A64_FMULX,
    A64_FDIV,
    A64_FADD,
    A64_FSUB,
    A64_FMAX,
    /// max(n, NaN) → exception or FPSR flag set
    A64_FMAXNM,
    /// max(n, NaN) → n
    A64_FMIN,
    /// min(n, NaN) → exception or FPSR flag set
    A64_FMINNM,
    /// min(n, NaN) → n

    /// Floating-Point Stepwise (scalar)
    A64_FRECPE,
    A64_FRECPS,
    A64_FRECPX,
    A64_FRSQRTE,
    A64_FRSQRTS,

    /// Floating-Point Fused Multiply (scalar)
    A64_FNMUL,
    A64_FMADD,
    A64_FMSUB,
    A64_FNMADD,
    A64_FNMSUB,

    /// Floating-Point Compare, Select, Move (scalar)
    A64_FCMP_REG,
    /// compare Rn, Rm
    A64_FCMP_ZERO,
    /// compare Rn and 0.0
    A64_FCMPE_REG,
    A64_FCMPE_ZERO,
    A64_FCCMP,
    A64_FCCMPE,
    A64_FCSEL,
    A64_FMOV_VEC2GPR,
    /// GPR ← SIMD&FP reg, without conversion
    A64_FMOV_GPR2VEC,
    /// GPR → SIMD&FP reg, ----
    A64_FMOV_TOP2GPR,
    /// GPR ← SIMD&FP top half (of full 128 bits), ----
    A64_FMOV_GPR2TOP,
    /// GPR → SIMD&FP top half (of full 128 bits), ----
    A64_FMOV_REG,
    /// SIMD&FP ←→ SIMD&FP
    A64_FMOV_IMM,
    /// SIMD&FP ← 8-bit float immediate (see VFPExpandImm)
    A64_FMOV_VEC,
    /// vector ← 8-bit imm ----; replicate imm to all lanes

    /// SIMD Floating-Point Compare
    A64_FCMEQ_REG,
    A64_FCMEQ_ZERO,
    A64_FCMGE_REG,
    A64_FCMGE_ZERO,
    A64_FCMGT_REG,
    A64_FCMGT_ZERO,
    A64_FCMLE_ZERO,
    A64_FCMLT_ZERO,
    A64_FACGE,
    A64_FACGT,

    /// SIMD Simple Floating-Point Computation (vector <op> vector, vector <op> vector[i])
    A64_FABS_VEC,
    A64_FABD_VEC,
    A64_FNEG_VEC,
    A64_FSQRT_VEC,
    A64_FMUL_ELEM,
    A64_FMUL_VEC,
    A64_FMULX_ELEM,
    A64_FMULX_VEC,
    A64_FDIV_VEC,
    A64_FADD_VEC,
    A64_FCADD,
    /// complex addition; Inst.imm := rotation in degrees (90, 270)
    A64_FSUB_VEC,
    A64_FMAX_VEC,
    A64_FMAXNM_VEC,
    A64_FMIN_VEC,
    A64_FMINNM_VEC,

    /// SIMD Floating-Point Stepwise
    A64_FRECPE_VEC,
    A64_FRECPS_VEC,
    A64_FRSQRTE_VEC,
    A64_FRSQRTS_VEC,

    /// SIMD Floating-Point Fused Multiply
    A64_FMLA_ELEM,
    A64_FMLA_VEC,
    A64_FMLAL_ELEM,
    A64_FMLAL_VEC,
    A64_FMLAL2_ELEM,
    A64_FMLAL2_VEC,
    A64_FCMLA_ELEM,
    /// Inst.imm := rotation in degrees (0, 90, 180, 270)
    A64_FCMLA_VEC,
    /// ---
    A64_FMLS_ELEM,
    A64_FMLS_VEC,
    A64_FMLSL_ELEM,
    A64_FMLSL_VEC,
    A64_FMLSL2_ELEM,
    A64_FMLSL2_VEC,

    /// SIMD Floating-Point Computation (reduce)
    A64_FADDP,
    A64_FADDP_VEC,
    A64_FMAXP,
    A64_FMAXP_VEC,
    A64_FMAXV,
    A64_FMAXNMP,
    A64_FMAXNMP_VEC,
    A64_FMAXNMV,
    A64_FMINP,
    A64_FMINP_VEC,
    A64_FMINV,
    A64_FMINNMP,
    A64_FMINNMP_VEC,
    A64_FMINNMV,

    /// SIMD Bitwise: Logical, Pop Count, Bit Reversal, Byte Swap, Shifts
    A64_AND_VEC,
    A64_BCAX,
    /// ARMv8.2-SHA
    A64_BIC_VEC_IMM,
    A64_BIC_VEC_REG,
    A64_BIF,
    A64_BIT,
    A64_BSL,
    A64_CLS_VEC,
    A64_CLZ_VEC,
    A64_CNT,
    A64_EOR_VEC,
    A64_EOR3,
    /// ARMv8.2-SHA
    A64_NOT_VEC,
    /// also called MVN
    A64_ORN_VEC,
    A64_ORR_VEC_IMM,
    A64_ORR_VEC_REG,
    A64_MOV_VEC,
    /// alias of ORR_VEC_REG
    A64_RAX1,
    /// ARMv8.2-SHA
    A64_RBIT_VEC,
    A64_REV16_VEC,
    A64_REV32_VEC,
    A64_REV64_VEC,
    A64_SHL_IMM,
    A64_SHL_REG,
    /// SSHL, USHL, SRSHL, URSHL
    A64_SHLL,
    /// SSHLL, USSHL
    A64_SHR,
    /// SSHR, USHR, SRSHR, URSHR
    A64_SHRN,
    /// SHRN, RSHRN
    A64_SRA,
    /// SSRA, USRA, SRSRA, URSRA
    A64_SLI,
    A64_SRI,
    A64_XAR,
    /// ARMv8.2-SHA

    /// SIMD Copy, Table Lookup, Transpose, Extract, Insert, Zip, Unzip
    ///
    /// Inst.imm := index i
    A64_DUP_ELEM,
    /// ∀k < lanes: Dst[k] ← Src[i] (or if Dst is scalar: Dst ← Src[i])
    A64_DUP_GPR,
    /// ∀k < lanes: Dst[k] ← Xn
    A64_EXT,
    A64_INS_ELEM,
    /// Dst[j] ← Src[i], (i, j stored in Inst.ins_elem)
    A64_INS_GPR,
    /// Dst[i] ← Xn
    A64_MOVI,
    /// includes MVNI
    A64_SMOV,
    /// Xd ← sext(Src[i])
    A64_UMOV,
    /// Xd ← Src[i]
    A64_TBL,
    /// Inst.imm := #regs of table ∈ {1,2,3,4}
    A64_TBX,
    /// ---
    A64_TRN1,
    A64_TRN2,
    A64_UZP1,
    A64_UZP2,
    A64_XTN,
    A64_ZIP1,
    A64_ZIP2,

    /// SIMD Integer/Bitwise Compare
    A64_CMEQ_REG,
    A64_CMEQ_ZERO,
    A64_CMGE_REG,
    A64_CMGE_ZERO,
    A64_CMGT_REG,
    A64_CMGT_ZERO,
    A64_CMHI_REG,
    /// no ZERO variant
    A64_CMHS_REG,
    /// no ZERO variant
    A64_CMLE_ZERO,
    /// no REG variant
    A64_CMLT_ZERO,
    /// no REG variant
    A64_CMTST,

    /// SIMD Integer Computation (vector <op> vector, vector <op> vector[i])
    ///
    /// Signedness (e.g. SABD vs UABD) is encoded via the SIMD_SIGNED flag,
    /// rounding vs truncating behaviour (e.g. SRSHL vs SSHL) in SIMD_ROUND.
    A64_ABS_VEC,

    A64_ABD,
    A64_ABDL,
    A64_ABA,
    A64_ABAL,

    A64_NEG_VEC,

    A64_MUL_ELEM,
    A64_MUL_VEC,
    A64_MULL_ELEM,
    A64_MULL_VEC,

    A64_ADD_VEC,
    A64_ADDHN,
    A64_ADDL,
    A64_ADDW,
    A64_HADD,

    A64_SUB_VEC,
    A64_SUBHN,
    A64_SUBL,
    A64_SUBW,
    A64_HSUB,

    A64_MAX_VEC,
    A64_MIN_VEC,

    A64_DOT_ELEM,
    A64_DOT_VEC,
    /// Inst.flags.vec = arrangement of destination (2s, 4s); sources are (8b, 16b)

    /// SIMD Integer Stepwise (both are unsigned exclusive)
    A64_URECPE,
    A64_URSQRTE,

    /// SIMD Integer Fused Multiply
    A64_MLA_ELEM,
    A64_MLA_VEC,
    A64_MLS_ELEM,
    A64_MLS_VEC,
    A64_MLAL_ELEM,
    /// SMLAL, UMLAL
    A64_MLAL_VEC,
    /// SMLAL, UMLAL
    A64_MLSL_ELEM,
    /// SMLSL, UMLSL
    A64_MLSL_VEC,
    /// SMLSL, UMLSL

    /// SIMD Integer Computation (reduce)
    A64_ADDP,
    /// Scalar; Dd ← Vn.d[1] + Vn.d[0]
    A64_ADDP_VEC,
    /// Concatenate Vn:Vm, then add pairwise and store result in Vd
    A64_ADDV,
    A64_ADALP,
    A64_ADDLP,
    A64_ADDLV,
    A64_MAXP,
    A64_MAXV,
    A64_MINP,
    A64_MINV,

    /// SIMD Saturating Integer Arithmetic (unsigned, signed)
    A64_QADD,
    A64_QABS,
    A64_SUQADD,
    A64_USQADD,
    A64_QSHL_IMM,
    A64_QSHL_REG,
    A64_QSHRN,
    A64_QSUB,
    A64_QXTN,

    /// SIMD Saturating Integer Arithmetic (signed exclusive)
    A64_SQABS,
    A64_SQADD,

    A64_SQDMLAL_ELEM,
    A64_SQDMLAL_VEC,
    A64_SQDMLSL_ELEM,
    A64_SQDMLSL_VEC,

    A64_SQDMULH_ELEM,
    /// SQDMULH, SQRDMULH
    A64_SQDMULH_VEC,
    /// SQDMULH, SQRDMULH
    A64_SQDMULL_ELEM,
    /// SQDMULL, SQRDMULL
    A64_SQDMULL_VEC,
    /// SQDMULL, SQRDMULL

    A64_SQNEG,

    /// Only these rounded variations exist
    A64_SQRDMLAH_ELEM,
    A64_SQRDMLAH_VEC,
    A64_SQRDMLSH_ELEM,
    A64_SQRDMLSH_VEC,

    A64_SQSHLU,
    A64_SQSHRUN,
    /// SQSHRUN, SQRSHRUN
    A64_SQXTUN,

    /// SIMD Polynomial Multiply
    A64_PMUL,
    A64_PMULL,
}

/// The condition bits used by conditial branches, selects and compares, stored in the
/// upper four bit of the Inst.flags field. The first three bits determine the condition
/// proper while the LSB inverts the condition if set.
pub mod Cond {
    /// =
    pub const COND_EQ: u8 = 0b0000;
    /// ≠
    pub const COND_NE: u8 = 0b0001;
    /// Carry Set
    pub const COND_CS: u8 = 0b0010;
    /// ≥, Unsigned (COND_HS)
    pub const COND_HS: u8 = 0b0010;
    /// Carry Clear
    pub const COND_CC: u8 = 0b0011;
    /// <, Unsigned (COND_LO)
    pub const COND_LO: u8 = 0b0011;
    /// < 0 (MInus)
    pub const COND_MI: u8 = 0b0100;
    /// ≥ 0 (PLus)
    pub const COND_PL: u8 = 0b0101;
    /// Signed Overflow
    pub const COND_VS: u8 = 0b0110;
    /// No Signed Overflow
    pub const COND_VC: u8 = 0b0111;
    /// >, Unsigned
    pub const COND_HI: u8 = 0b1000;
    /// ≤, Unsigned
    pub const COND_LS: u8 = 0b1001;
    /// ≥, Signed
    pub const COND_GE: u8 = 0b1010;
    /// <, Signed
    pub const COND_LT: u8 = 0b1011;
    /// >, Signed
    pub const COND_GT: u8 = 0b1100;
    /// ≤, Signed
    pub const COND_LE: u8 = 0b1101;
    /// Always true
    pub const COND_AL: u8 = 0b1110;
    /// Always true (not "never" as in A32!)
    pub const COND_NV: u8 = 0b1111;
}

pub mod Shift {
    pub const SH_LSL: u8 = 0b00;
    pub const SH_LSR: u8 = 0b01;
    pub const SH_ASR: u8 = 0b10;
    pub const SH_ROR: u8 = 0b11;
    // only for RORV instruction; shifted add/sub does not support it
    pub const SH_RESERVED: u8 = SH_ROR;
}

/// Addressing modes, stored in the top three bits of the flags field
/// (where the condition is stored for conditional instructions). See
/// page 187, section C1.3.3 of the 2020 ARM ARM for ARMv8.
///
/// The base register is stored in the Inst.ldst.rn field.
///
/// The LSL amount for the REG and EXT depends on the access size
/// (#4 for 128 bits (SIMD), #3 for 64 bits, #2 for 32 bits, #1 for
/// 16 bits, #0 for 8 bits) and is used for array indexing:
///
///     u64 a[128];
///     u64 x0 = a[i]; → ldr x0, [a, i, LSL #3]
///
pub mod AddrMode {
    /// [base] -- used by atomics, exclusive, ordered load/stores → check Inst.ldst_order
    pub const AM_SIMPLE: u8 = 0;
    /// [base, #imm]
    pub const AM_OFF_IMM: u8 = 1;
    /// [base, Xm, {LSL #imm}] (#imm either #log2(size) or #0)
    pub const AM_OFF_REG: u8 = 2;
    /// [base, Wm, {S|U}XTW {#imm}] (#imm either #log2(size) or #0)
    pub const AM_OFF_EXT: u8 = 3;
    /// [base, #imm]!
    pub const AM_PRE: u8 = 4;
    /// [base],#imm  (for LDx, STx also register: [base],Xm)
    pub const AM_POST: u8 = 5;
    /// label
    pub const AM_LITERAL: u8 = 6;
}

/// Memory ordering semantics for Atomic instructions and the Load/Stores in the
/// Exclusive group.
#[derive(Clone)]
pub enum MemOrdering {
    MO_NONE,
    /// Load-Acquire -- sequentially consistent Acquire
    MO_ACQUIRE,
    /// Load-LOAcquire -- Acquire in Limited Ordering Region (LORegion)
    MO_LO_ACQUIRE,
    /// Load-AcquirePC -- weaker processor consistent (PC) Acquire
    MO_ACQUIRE_PC,
    /// Store-Release
    MO_RELEASE,
    /// Store-LORelease -- Release in LORegion
    MO_LO_RELEASE,
}

/// Size, encoded in two bits.
pub mod Size {
    /// Byte     -  8 bit
    pub const SZ_B: u8 = 0b00;
    /// Halfword - 16 bit
    pub const SZ_H: u8 = 0b01;
    /// Word     - 32 bit
    pub const SZ_W: u8 = 0b10;
    /// Extended - 64 bit
    pub const SZ_X: u8 = 0b11;
}

/// Floating-point size, encoded in three bits. Mostly synonymous to Size, but
/// with the 128-bit quadruple precision.
pub mod FPSize {
    use crate::aarch64_reader::Size;

    /// Byte   -   8 bits
    pub const FSZ_B: u8 = Size::SZ_B;
    /// Half   -  16 bits
    pub const FSZ_H: u8 = Size::SZ_H;
    /// Single -  32 bits
    pub const FSZ_S: u8 = Size::SZ_W;
    /// Double -  64 bits
    pub const FSZ_D: u8 = Size::SZ_X;

    // "Virtual" encoding, never used in the actual instructions.
    // There, Quad precision is encoded in various incoherent ways.
    /// Quad   - 128 bits
    pub const FSZ_Q: u8 = 0b111;
}

/// The three-bit Vector Arrangement specifier determines the structure of the
/// vectors used by a SIMD instruction, where it is encoded in size(2):Q(1).
///
/// The vector registers V0...V31 are 128 bit long, but some arrangements use
/// only the bottom 64 bits. Scalar SIMD instructions encode their scalars'
/// precision as FPSize in the upper two bits.
pub mod VectorArrangement {
    use crate::aarch64_reader::FPSize;

    ///  64 bit
    pub const VA_8B: u8 = (FPSize::FSZ_B << 1) | 0;
    /// 128 bit
    pub const VA_16B: u8 = (FPSize::FSZ_B << 1) | 1;
    ///  64 bit
    pub const VA_4H: u8 = (FPSize::FSZ_H << 1) | 0;
    /// 128 bit
    pub const VA_8H: u8 = (FPSize::FSZ_H << 1) | 1;
    ///  64 bit
    pub const VA_2S: u8 = (FPSize::FSZ_S << 1) | 0;
    /// 128 bit
    pub const VA_4S: u8 = (FPSize::FSZ_S << 1) | 1;
    ///  64 bit
    pub const VA_1D: u8 = (FPSize::FSZ_D << 1) | 0;
    /// 128 bit
    pub const VA_2D: u8 = (FPSize::FSZ_D << 1) | 1;
}

/// Floating-point rounding mode. See shared/functions/float/fprounding/FPRounding
/// in the shared pseudocode functions of the A64 ISA documentation. The letter
/// is the one used in the FCVT* mnemonics.
#[derive(Clone)]
pub enum FPRounding {
    /// "Current rounding mode"
    FPR_CURRENT,
    /// N, Nearest with Ties to Even, default IEEE 754 mode
    FPR_TIE_EVEN,
    /// A, Nearest with Ties Away from Zero
    FPR_TIE_AWAY,
    /// M, → -∞
    FPR_NEG_INF,
    /// Z, → 0
    FPR_ZERO,
    /// P, → +∞
    FPR_POS_INF,
    /// XN, Non-IEEE 754 Round to Odd, only used by FCVTXN(2)
    FPR_ODD,
}

/// ExtendType: signed(1):size(2)
pub mod ExtendType {
    use crate::aarch64_reader::Size;

    pub const UXTB: u8 = (0 << 2) | Size::SZ_B;
    pub const UXTH: u8 = (0 << 2) | Size::SZ_H;
    pub const UXTW: u8 = (0 << 2) | Size::SZ_W;
    pub const UXTX: u8 = (0 << 2) | Size::SZ_X;
    pub const SXTB: u8 = (1 << 2) | Size::SZ_B;
    pub const SXTH: u8 = (1 << 2) | Size::SZ_H;
    pub const SXTW: u8 = (1 << 2) | Size::SZ_W;
    pub const SXTX: u8 = (1 << 2) | Size::SZ_X;
}

/// PstateField: encodes which PSTATE bits the MSR_IMM instruction modifies.
#[derive(Clone)]
pub enum PStateField {
    PSF_UAO,
    PSF_PAN,
    PSF_SPSel,
    PSF_SSBS,
    PSF_DIT,
    PSF_DAIFSet,
    PSF_DAIFClr,
}

pub mod FlagMasks {
    /// use the 32-bit W0...W31 facets?
    pub const W32: u8 = 1 << 0;
    /// modify the NZCV flags? (S mnemonic suffix)
    pub const SET_FLAGS: u8 = 1 << 1;
    /// SIMD: Is scalar? If so, interpret Inst.flags.vec<2:1> as FPSize precision for the scalar.
    pub const SIMD_SCALAR: u8 = 1 << 5;
    /// Integer SIMD: treat values as signed?
    pub const SIMD_SIGNED: u8 = 1 << 6;
    /// Integer SIMD: round result instead of truncating?
    pub const SIMD_ROUND: u8 = 1 << 7;
}

#[derive(Clone)]
pub struct Movk {
    imm16: u32,
    lsl: u32,
}

#[derive(Clone)]
pub struct Bfm {
    lsb: u32,
    width: u32,
}

#[derive(Clone)]
pub struct Ccmp {
    nzcv: u32,
    imm5: u32,
}

#[derive(Clone)]
pub struct Sys {
    op1: u16,
    op2: u16,
    crn: u16,
    crm: u16,
}

#[derive(Clone)]
pub struct MsrImm {
    psfld: u32,
    imm: u32,
}

#[derive(Clone)]
pub struct Tbz {
    offset: i32,
    bit: u32,
}

#[derive(Clone)]
pub struct InstShift {
    typ: u32,
    amount: u32,
}

#[derive(Clone)]
pub struct Rmif {
    mask: u32,
    ror: u32,
}

#[derive(Clone)]
pub struct Extend {
    typ: u32,
    lsl: u32,
}

#[derive(Clone)]
pub struct LdstOrder {
    load: u16,
    store: u16,
    rs: u8,
}

#[derive(Clone)]
pub struct SimdLdst {
    nreg: u32,
    index: u16,
    offset: i16,
}

#[derive(Clone)]
pub struct Fcvt {
    mode: u32,
    fbits: u16,
    sgn: u16,
}

#[derive(Clone)]
pub struct Frint {
    mode: u32,
    bits: u32,
}

#[derive(Clone)]
pub struct InsElem {
    dst: u32,
    src: u32,
}

#[derive(Clone)]
pub struct FcmlaElem {
    idx: u32,
    rot: u32,
}

#[derive(Clone)]
pub struct Inst {
    op: Op,
    flags: u8,
    rd: u8,
    rn: u8,
    rm: u8,
    rt2: u8,
    rs: u8,
    imm: u64,
    fimm: f64,
    offset: i64,
    ra: u8,
    error: String,
    movk: Movk,
    bfm: Bfm,
    ccmp: Ccmp,
    sys: Sys,
    msr_imm: MsrImm,
    tbz: Tbz,
    shift: u8,
    rmif: Rmif,
    extend: Extend,
    ldst_order: LdstOrder,
    simd_ldst: SimdLdst,
    fcvt: Fcvt,
    frint: Frint,
    ins_elem: InsElem,
    fcmla_elem: FcmlaElem,
}

const UNKNOWN_INST: Inst = Inst {
    op: Op::A64_UNKNOWN,
    flags: 0,
    rd: 0,
    rn: 0,
    rm: 0,
    rt2: 0,
    rs: 0,
    imm: 0,
    fimm: 0.0,
    offset: 0,
    ra: 0,
    error: String::new(),
    movk: Movk { imm16: 0, lsl: 0 },
    bfm: Bfm { lsb: 0, width: 0 },
    ccmp: Ccmp { nzcv: 0, imm5: 0 },
    sys: Sys {
        op1: 0,
        op2: 0,
        crn: 0,
        crm: 0,
    },
    msr_imm: MsrImm { psfld: 0, imm: 0 },
    tbz: Tbz { offset: 0, bit: 0 },
    shift: Shift::SH_LSL,
    rmif: Rmif { mask: 0, ror: 0 },
    extend: Extend { typ: 0, lsl: 0 },
    ldst_order: LdstOrder {
        load: 0,
        store: 0,
        rs: Registries::ZERO_REG,
    },
    simd_ldst: SimdLdst {
        nreg: 0,
        index: 0,
        offset: 0,
    },
    fcvt: Fcvt {
        mode: 0,
        fbits: 0,
        sgn: 0,
    },
    frint: Frint { mode: 0, bits: 0 },
    ins_elem: InsElem { dst: 0, src: 0 },
    fcmla_elem: FcmlaElem { idx: 0, rot: 0 },
};

pub fn errinst(err: String) -> Inst {
    let mut inst = UNKNOWN_INST;
    inst.op = Op::A64_ERROR;
    inst.error = err;
    return inst;
}

pub fn fad_get_cond(flags: u8) -> u8 {
    return (flags >> 4) & 0b1111;
}

fn set_cond(flags: u8, cond: u8) -> u8 {
    let cond = cond as u8 & 0xF;
    let flags = flags & 0x0F;
    (cond << 4) | flags
}

pub fn invert_cond(flags: u8) -> u8 {
    let cond = fad_get_cond(flags);
    return set_cond(flags, cond as u8 ^ 0b001); // invert LSB
}

// Addressing mode, for Loads and Stores.
pub fn fad_get_addrmode(flags: u8) -> u8 {
    return (flags >> 5) & 0b111;
}

pub fn set_addrmode(flags: u8, mode: u8) -> u8 {
    return ((mode & 0b111) << 5) | (flags & 0b11111);
}

// How much memory to load/store (access size) and whether to sign-
// or zero-extend the value.
pub fn fad_get_mem_extend(flags: u8) -> u8 {
    return (flags >> 2) & 0b111;
}

pub fn set_mem_extend(flags: u8, memext: u8) -> u8 {
    return ((memext & 0b111) << 2) | (flags & 0b11100011);
}

pub fn fad_get_vec_arrangement(flags: u8) -> u8 {
    return (flags >> 2) & 0b111;
}

pub fn set_vec_arrangement(flags: u8, va: u8) -> u8 {
    return ((va & 0b111) << 2) | (flags & 0b11100011);
}

pub fn fad_get_prec(flags: u8) -> u8 {
    return (flags >> 1) & 0b111;
}

pub fn set_prec(flags: u8, prec: u8) -> u8 {
    return ((prec & 0b111) << 1) | (flags & 0b11110001);
}

pub fn fad_size_from_vec_arrangement(va: u8) -> u8 {
    return va >> 1;
}

// The destination register Rd, if present, occupies bits 0..4.
// Register 31 is treated as the Zero/Discard register ZR/WZR.
pub fn regRd(binst: u32) -> u8 {
    return (binst & 0b11111) as u8;
}

// Register 31 is treated as the stack pointer SP.
pub fn regRdSP(binst: u32) -> u8 {
    let rd = binst & 0b11111;
    return if rd == 31 { Registries::STACK_POINTER } else { rd.try_into().unwrap() };
}

// The first operand register Rn, if present, occupies bits 5..9.
// Register 31 is treated as the Zero/Discard register ZR/WZR.
pub fn regRn(binst: u32) -> u8 {
    return ((binst >> 5) & 0b11111).try_into().unwrap();
}

// Register 31 is treated as the stack pointer SP.
pub fn regRnSP(binst: u32) -> u8 {
    let rn = (binst >> 5) & 0b11111;
    return if rn == 31 { Registries::STACK_POINTER } else { rn.try_into().unwrap() };
}

// The second operand register Rm, if present, occupies bits 16..20.
// Register 31 is treated as the Zero/Discard register ZR/WZR.
pub fn regRm(binst: u32) -> u8 {
    return ((binst >> 16) & 0b11111).try_into().unwrap();
}

// Register 31 is treated as the stack pointer SP.
pub fn regRmSP(binst: u32) -> u8 {
    let rm = (binst >> 16) & 0b11111;
    return if rm == 31 { Registries::STACK_POINTER } else { rm.try_into().unwrap() };
}

// sext sign-extends the b-bits number in x to 64 bit. The upper (64-b) bits
// must be zero. Seldom needed, but fiddly.
//
// Taken from https://graphics.stanford.edu/~seander/bithacks.html#VariableSignExtend
pub fn sext(x: u64, b: u8) -> i64 {
    let mask = (1 as i64) << (b - 1);
    return ((x as i64) ^ mask) - mask;
}

enum OpKind {
    Unknown,
    PCRelAddr,
    AddSubTags,
    AddSub,
    Logic,
    Move,
    Bitfield,
    Extract,
}

pub fn data_proc_imm(binst: u32) -> Inst {
    let mut inst = UNKNOWN_INST.clone();

    let op01 = (binst >> 22) & 0b1111; // op0 and op1 together
    let top3 = (binst >> 29) & 0b111;

    let kind = match op01 {
        0b0000 | 0b0001 | 0b0010 | 0b0011 => PCRelAddr, // 00xx
        0b0110 | 0b0111 => AddSubTags, // 011x
        0b0100 | 0b0101 => AddSub, // 010x
        0b1000 | 0b1001 => Logic, // 100x
        0b1010 | 0b1011 => Move, // 101x
        0b1100 | 0b1101 => Bitfield, // 110x
        0b1110 | 0b1111 => Extract, // 111x
        _ => {
            println!("Unknown Operator Kind {}", op01);
            Unknown
        }
    };

    // Bit 31 (sf) controls length of registers (0 → 32 bit, 1 → 64 bit)
    // for most of these data processing operations.
    if (top3 & 0b100) == 0 {
        inst.flags |= W32;
    }

    match kind {
        Unknown => return UNKNOWN_INST,
        PCRelAddr => {
            if (top3 & 0b100) == 0 {
                inst.op = A64_ADR;
            } else {
                inst.op = A64_ADRP;
            }

            inst.flags &= !W32; // no 32-bit variant of these

            // First, just extract the immediate.
            let immhi: u64 = (binst & (0b1111111111111111111 << 5)) as u64;
            let immlo: u64 = (top3 & 0b011).into();
            let uimm = (immhi >> (5 - 2)) | immlo; // pos(immhi) = 5; 2: len(immlo)

            let scale: u64 = if inst.op == A64_ADRP { 4096 } else { 1 }; // ADRP: Page (4K) Address
            let simm: i64 = scale as i64 * sext(uimm, 21); // PC-relative → signed
            inst.offset = simm;

            inst.rd = regRd(binst);
        }
        AddSubTags => panic!("ADDG, SUBG not supported"),
        AddSub => {
            let is_add = (top3 & 0b010) == 0;
            inst.op = if is_add { A64_ADD_IMM } else { A64_SUB_IMM };
            if (top3 & 0b001) != 0 {
                inst.flags |= SET_FLAGS;
            }

            let unshifted_imm: u64 = ((binst >> 10) & 0b111111111111) as u64;
            let shift_by_12 = (binst & (1 << 22)) > 0;
            inst.imm = if shift_by_12 { unshifted_imm << 12 } else { unshifted_imm };

            // ADDS/SUBS and thus CMN/CMP interpret R31 as the zero register,
            // while normal ADD and SUB treat it as the stack pointer.
            inst.rd = if inst.flags & SET_FLAGS != 0 { regRd(binst) } else { regRdSP(binst) };
            inst.rn = regRnSP(binst);

            if inst.rd == ZERO_REG && (inst.flags & SET_FLAGS) != 0 {
                match inst.op {
                    A64_ADD_IMM => inst.op = A64_CMN_IMM,
                    A64_SUB_IMM => inst.op = A64_CMP_IMM,
                    _ => {} // impossible
                }
            } else if inst.op == A64_ADD_IMM && !shift_by_12 && unshifted_imm == 0 && ((inst.rd == STACK_POINTER) || (inst.rn == STACK_POINTER)) {
                inst.op = A64_MOV_SP;
            }
        }
        Logic => {
            match top3 & 0b011 {
                0b00 => inst.op = A64_AND_IMM,
                0b01 => inst.op = A64_ORR_IMM,
                0b10 => inst.op = A64_EOR_IMM,
                0b11 => {
                    inst.op = if regRd(binst) == ZERO_REG { A64_TST_IMM } else { A64_AND_IMM };
                    inst.flags |= SET_FLAGS;
                }
                _ => panic!("Unexpected Logic Operator {}", top3 & 0b011),
            }

            let immr: u8 = ((binst >> 16) & 0b111111) as u8;
            let imms: u8 = ((binst >> 10) & 0b111111) as u8;
            let N: u8 = if inst.flags & W32 != 0 { 0 } else { ((binst >> 22) & 1) as u8 }; // N is part of imm for 64-bit variants
            inst.imm = decode_bitmask(N, imms, immr, inst.flags & W32 != 0);

            // ANDS and by extension TST interpret R31 as the zero register, while
            // regular immediate AND interprets it as the stack pointer.
            inst.rd = if inst.flags & SET_FLAGS != 0 { regRd(binst) } else { regRdSP(binst) };
            inst.rn = regRn(binst);
        }
        Move => {
            let hw: u8 = ((binst >> 21) & 0b11) as u8;
            let shift: u8 = (16 * hw) as u8;
            let imm16: u64 = ((binst >> 5) & 0xFFFF) as u64;

            match top3 & 0b011 {
                0b00 => { // MOVN: Move with NOT
                    inst.op = A64_MOV_IMM;
                    inst.imm = !(imm16 << shift);
                }
                0b01 => return UNKNOWN_INST,
                0b10 => { // MOVZ: zero other bits
                    inst.op = A64_MOV_IMM;
                    inst.imm = imm16 << shift;
                }
                0b11 => {// MOVK: keep other bits
                    inst.op = A64_MOVK;
                    inst.movk.imm16 = imm16 as u32;
                    inst.movk.lsl = shift as u32;
                }
                _ => {}
            }

            inst.rd = regRd(binst);
        }
        Bitfield => {
            let op = match top3 & 0b011 { // base instructions
                0b00 => A64_SBFM,
                0b01 => A64_BFM,
                0b10 => A64_UBFM,
                _ => panic!("data_proc_imm/Bitfield: neither SBFM, BFM or UBFM")
            };

            let w32 = (inst.flags & W32) != 0;
            let immr: u8 = ((binst >> 16) & 0b111111) as u8;
            let imms: u8 = ((binst >> 10) & 0b111111) as u8;
            let rd = regRd(binst);
            let rn = regRn(binst);
            inst = find_bfm_alias(op, w32, rd, rn, immr, imms);
        }
        Extract => {
            inst.op = A64_EXTR;
            inst.imm = ((binst >> 10) & 0b111111) as u64;
            inst.rd = regRd(binst);
            inst.rn = regRn(binst);
            inst.rm = regRm(binst);

            if inst.rn == inst.rm {
                inst.op = A64_ROR_IMM;
                inst.rm = 0; // unused for ROR_IMM → clear again
            }
        }
    }

    inst
}


/// Returns the 0-based index of the highest bit. Should be compiled down
/// to a single native instruction.
fn highest_bit(mut x: u32) -> i32 {
    let mut n = 0;
    while x != 1 {
        x >>= 1;
        n += 1;
    }
    return n;
}

/// Rotate the len-bit number x n places to the right. Based on the first
/// example at https://en.wikipedia.org/wiki/Bitwise_operation#Circular_shifts
/// (except turned around, to make it rotate right).
fn ror(x: u64, n: u32, len: u32) -> u64 {
    let raw = (x >> n) | (x << (len - n));
    if len == 64 {
        return raw;
    }
    return raw & ((1u64 << len) - 1); // truncate left side to len bits
}


/// Implementation of the A64 pseudocode function DecodeBitMasks (pp. 1683-1684).
///
/// The logical immediate instructions encode 32-bit or 64-bit masks using merely
/// 12 or 13 bits. We want the decoded mask in our Inst.imm field. We only need
/// the "wmask" of DecodeBitMasks, so return only that.
fn decode_bitmask(immN: u8, imms: u8, immr: u8, w32: bool) -> u64 {
    let M: u32 = if w32 { 32 } else { 64 };

    // Guarantee it's only the number of bits in the pseudocode signature.
    let immN = immN & 1;
    let imms = imms & 0b111111;
    let immr = immr & 0b111111;

    // length of bitmask (1..6)
    let len = highest_bit(((immN << 6) | ((!imms) & 0b111111)) as u32);

    // 1..6 consecutive ones, basis of pattern
    let mut levels = 0;
    for i in 0..len {
        levels = (levels << 1) | 1;
    }

    let S: u32 = (imms & levels) as u32;
    let R: u32 = (immr & levels) as u32;
    let esize = 1 << len; // 2, 4, 8, 16, 32, 64

    // welem: pattern of 1s then zero-extended to esize
    // e.g. esize = 8; S+1 = 4 → welem = 0b00001111
    let mut welem = 0;
    for _ in 0..(S + 1) {
        welem = (welem << 1) | 1;
    }

    // wmask = Replicate(ROR(welem, R));
    welem = ror(welem, R, esize);
    let mut wmask = 0;
    for i in (0..M).step_by(esize as usize) {
        wmask = (wmask << esize) | welem;
    }

    return wmask;
}

fn find_bfm_alias(op: Op, w32: bool, rd: u8, rn: u8, immr: u8, imms: u8) -> Inst {
    let mut inst = UNKNOWN_INST;
    let all_ones: u8 = if w32 { 31 } else { 63 }; // u8
    let bits: u8 = if w32 { 32 } else { 64 }; // u8

    inst.rd = rd;
    inst.rn = rn;
    if w32 {
        inst.flags |= W32;
    }

    if op == A64_BFM {
        if imms >= immr {
            inst.op = A64_BFXIL;
            inst.bfm.lsb = immr as u32;
            inst.bfm.width = (imms - immr + 1) as u32;
            return inst;
        }

        inst.op = if rn == ZERO_REG { A64_BFC } else { A64_BFI };
        inst.bfm.lsb = (bits - immr) as u32;
        inst.bfm.width = (imms + 1) as u32;
        return inst;
    }

    let sign = op == A64_SBFM;

    if !sign && imms as u8 + 1 == immr as u8 && imms as u8 != all_ones {
        inst.op = A64_LSL_IMM;
        inst.imm = (all_ones - imms) as u64;
        return inst;
    }

    if imms as u8 == all_ones {
        inst.op = if sign { A64_ASR_IMM } else { A64_LSR_IMM };
        inst.imm = immr as u64;
        return inst;
    }

    if imms < immr {
        inst.op = if sign { A64_SBFIZ } else { A64_UBFIZ };
        inst.bfm.lsb = (bits - immr) as u32;
        inst.bfm.width = (imms + 1) as u32;
        return inst;
    }

    if immr == 0 {
        match imms {
            7 => {
                inst.op = A64_EXTEND;
                inst.extend.typ = if sign { SXTB } else { UXTB } as u32;
                return inst;
            }
            15 => {
                inst.op = A64_EXTEND;
                inst.extend.typ = if sign { SXTH } else { UXTH } as u32;
                return inst;
            }
            31 => {
                inst.op = A64_EXTEND;
                inst.extend.typ = SXTW as u32;
                return if sign { inst } else { panic!("find_bfm_alias: there is no UXTW instruction") };
            }
            _ => {}
        }
    }

    inst.op = if sign { A64_SBFX } else { A64_UBFX };
    inst.bfm.lsb = immr as u32;
    inst.bfm.width = (imms - immr + 1) as u32;
    inst
}