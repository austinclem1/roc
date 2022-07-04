use crate::subs::Variable;
use roc_module::symbol::Symbol;

/// A bound placed on a number because of its literal value.
/// e.g. `-5` cannot be unsigned, and 300 does not fit in a U8
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumericRange {
    IntAtLeastSigned(IntLitWidth),
    IntAtLeastEitherSign(IntLitWidth),
    NumAtLeastSigned(IntLitWidth),
    NumAtLeastEitherSign(IntLitWidth),
}

impl NumericRange {
    pub fn contains_symbol(&self, symbol: Symbol) -> Option<bool> {
        let contains = match symbol {
            Symbol::NUM_I8 => self.contains_int_width(IntLitWidth::I8),
            Symbol::NUM_U8 => self.contains_int_width(IntLitWidth::U8),
            Symbol::NUM_I16 => self.contains_int_width(IntLitWidth::I16),
            Symbol::NUM_U16 => self.contains_int_width(IntLitWidth::U16),
            Symbol::NUM_I32 => self.contains_int_width(IntLitWidth::I32),
            Symbol::NUM_U32 => self.contains_int_width(IntLitWidth::U32),
            Symbol::NUM_I64 => self.contains_int_width(IntLitWidth::I64),
            Symbol::NUM_NAT => self.contains_int_width(IntLitWidth::Nat),
            Symbol::NUM_U64 => self.contains_int_width(IntLitWidth::U64),
            Symbol::NUM_I128 => self.contains_int_width(IntLitWidth::I128),
            Symbol::NUM_U128 => self.contains_int_width(IntLitWidth::U128),

            Symbol::NUM_DEC => self.contains_float_width(FloatWidth::Dec),
            Symbol::NUM_F32 => self.contains_float_width(FloatWidth::F32),
            Symbol::NUM_F64 => self.contains_float_width(FloatWidth::F64),

            Symbol::NUM_NUM | Symbol::NUM_INT | Symbol::NUM_FRAC => {
                // these satisfy any range that they are given
                true
            }

            _ => {
                return None;
            }
        };

        Some(contains)
    }

    fn contains_float_width(&self, _width: FloatWidth) -> bool {
        // we don't currently check the float width
        true
    }

    fn contains_int_width(&self, width: IntLitWidth) -> bool {
        use NumericRange::*;

        let (range_signedness, at_least_width) = match self {
            IntAtLeastSigned(width) => (SignDemand::Signed, width),
            IntAtLeastEitherSign(width) => (SignDemand::NoDemand, width),
            NumAtLeastSigned(width) => (SignDemand::Signed, width),
            NumAtLeastEitherSign(width) => (SignDemand::NoDemand, width),
        };

        let (actual_signedness, _) = width.signedness_and_width();

        if let (IntSignedness::Unsigned, SignDemand::Signed) = (actual_signedness, range_signedness)
        {
            return false;
        }

        width.signedness_and_width().1 >= at_least_width.signedness_and_width().1
    }

    fn width(&self) -> IntLitWidth {
        use NumericRange::*;
        match self {
            IntAtLeastSigned(w)
            | IntAtLeastEitherSign(w)
            | NumAtLeastSigned(w)
            | NumAtLeastEitherSign(w) => *w,
        }
    }

    /// Returns the intersection of `self` and `other`, i.e. the greatest lower bound of both, or
    /// `None` if there is no common lower bound.
    pub fn intersection(&self, other: &Self) -> Option<Self> {
        use NumericRange::*;
        let (left, right) = (self.width(), other.width());
        let constructor: fn(IntLitWidth) -> NumericRange = match (self, other) {
            // Matching against a signed int, the intersection must also be a signed int
            (IntAtLeastSigned(_), _) | (_, IntAtLeastSigned(_)) => IntAtLeastSigned,
            // It's a signed number, but also an int, so the intersection must be a signed int
            (NumAtLeastSigned(_), IntAtLeastEitherSign(_))
            | (IntAtLeastEitherSign(_), NumAtLeastSigned(_)) => IntAtLeastSigned,
            //  It's a signed number
            (NumAtLeastSigned(_), NumAtLeastSigned(_) | NumAtLeastEitherSign(_))
            | (NumAtLeastEitherSign(_), NumAtLeastSigned(_)) => NumAtLeastSigned,
            // Otherwise we must be an int, signed or unsigned
            (IntAtLeastEitherSign(_), IntAtLeastEitherSign(_) | NumAtLeastEitherSign(_))
            | (NumAtLeastEitherSign(_), IntAtLeastEitherSign(_)) => IntAtLeastEitherSign,
            // Otherwise we must be a num, signed or unsigned
            (NumAtLeastEitherSign(_), NumAtLeastEitherSign(_)) => NumAtLeastEitherSign,
        };

        // One is a superset of the other if it's a superset on both sides
        if left.is_superset(&right, true) && left.is_superset(&right, false) {
            Some(constructor(left))
        } else if right.is_superset(&left, true) && right.is_superset(&left, false) {
            Some(constructor(right))
        } else {
            None
        }
    }

    pub fn variable_slice(&self) -> &'static [Variable] {
        use NumericRange::*;

        match self {
            IntAtLeastSigned(width) => {
                let target = int_lit_width_to_variable(*width);
                let start = SIGNED_VARIABLES.iter().position(|v| *v == target).unwrap();
                let end = SIGNED_VARIABLES.len() - 3;

                &SIGNED_VARIABLES[start..end]
            }
            IntAtLeastEitherSign(width) => {
                let target = int_lit_width_to_variable(*width);
                let start = ALL_VARIABLES.iter().position(|v| *v == target).unwrap();
                let end = ALL_VARIABLES.len() - 3;

                &ALL_VARIABLES[start..end]
            }
            NumAtLeastSigned(width) => {
                let target = int_lit_width_to_variable(*width);
                let start = SIGNED_VARIABLES.iter().position(|v| *v == target).unwrap();

                &SIGNED_VARIABLES[start..]
            }
            NumAtLeastEitherSign(width) => {
                let target = int_lit_width_to_variable(*width);
                let start = ALL_VARIABLES.iter().position(|v| *v == target).unwrap();

                &ALL_VARIABLES[start..]
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum IntSignedness {
    Unsigned,
    Signed,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IntLitWidth {
    U8,
    U16,
    U32,
    U64,
    U128,
    I8,
    I16,
    I32,
    I64,
    I128,
    Nat,
    // An int literal can be promoted to an f32/f64/Dec if appropriate. The respective widths for
    // integers that can be stored in these float types without losing precision are:
    //   f32: +/- 2^24
    //   f64: +/- 2^53
    //   dec: Int128::MAX/Int128::MIN
    F32,
    F64,
    Dec,
}

impl IntLitWidth {
    /// Returns the `IntSignedness` and bit width of a variant.
    fn signedness_and_width(&self) -> (IntSignedness, u32) {
        use IntLitWidth::*;
        use IntSignedness::*;
        match self {
            U8 => (Unsigned, 8),
            U16 => (Unsigned, 16),
            U32 => (Unsigned, 32),
            U64 => (Unsigned, 64),
            U128 => (Unsigned, 128),
            I8 => (Signed, 8),
            I16 => (Signed, 16),
            I32 => (Signed, 32),
            I64 => (Signed, 64),
            I128 => (Signed, 128),
            // TODO: Nat is platform specific!
            Nat => (Unsigned, 64),
            F32 => (Signed, 24),
            F64 => (Signed, 53),
            Dec => (Signed, 128),
        }
    }

    pub fn type_str(&self) -> &'static str {
        use IntLitWidth::*;
        match self {
            U8 => "U8",
            U16 => "U16",
            U32 => "U32",
            U64 => "U64",
            U128 => "U128",
            I8 => "I8",
            I16 => "I16",
            I32 => "I32",
            I64 => "I64",
            I128 => "I128",
            Nat => "Nat",
            F32 => "F32",
            F64 => "F64",
            Dec => "Dec",
        }
    }

    pub fn max_value(&self) -> u128 {
        use IntLitWidth::*;
        match self {
            U8 => u8::MAX as u128,
            U16 => u16::MAX as u128,
            U32 => u32::MAX as u128,
            U64 => u64::MAX as u128,
            U128 => u128::MAX,
            I8 => i8::MAX as u128,
            I16 => i16::MAX as u128,
            I32 => i32::MAX as u128,
            I64 => i64::MAX as u128,
            I128 => i128::MAX as u128,
            // TODO: this is platform specific!
            Nat => u64::MAX as u128,
            // Max int value without losing precision: 2^24
            F32 => 16_777_216,
            // Max int value without losing precision: 2^53
            F64 => 9_007_199_254_740_992,
            // Max int value without losing precision: I128::MAX
            Dec => i128::MAX as u128,
        }
    }

    pub fn min_value(&self) -> i128 {
        use IntLitWidth::*;
        match self {
            U8 | U16 | U32 | U64 | U128 | Nat => 0,
            I8 => i8::MIN as i128,
            I16 => i16::MIN as i128,
            I32 => i32::MIN as i128,
            I64 => i64::MIN as i128,
            I128 => i128::MIN,
            // Min int value without losing precision: -2^24
            F32 => -16_777_216,
            // Min int value without losing precision: -2^53
            F64 => -9_007_199_254_740_992,
            // Min int value without losing precision: I128::MIN
            Dec => i128::MIN,
        }
    }

    /// Checks if `self` represents superset of integers that `lower_bound` represents, on a particular
    /// side of the integers relative to 0.
    ///
    /// If `is_negative` is true, the negative side is checked; otherwise the positive side is checked.
    pub fn is_superset(&self, lower_bound: &Self, is_negative: bool) -> bool {
        use IntSignedness::*;

        if is_negative {
            match (
                self.signedness_and_width(),
                lower_bound.signedness_and_width(),
            ) {
                ((Signed, us), (Signed, lower_bound)) => us >= lower_bound,
                // Unsigned ints can never represent negative numbers; signed (non-zero width)
                // ints always can.
                ((Unsigned, _), (Signed, _)) => false,
                ((Signed, _), (Unsigned, _)) => true,
                // Trivially true; both can only express 0.
                ((Unsigned, _), (Unsigned, _)) => true,
            }
        } else {
            match (
                self.signedness_and_width(),
                lower_bound.signedness_and_width(),
            ) {
                ((Signed, us), (Signed, lower_bound))
                | ((Unsigned, us), (Unsigned, lower_bound)) => us >= lower_bound,

                // Unsigned ints with the same bit width as their unsigned counterparts can always
                // express 2x more integers on the positive side as unsigned ints.
                ((Unsigned, us), (Signed, lower_bound)) => us >= lower_bound,

                // ...but that means signed int widths can represent less than their unsigned
                // counterparts, so the below is true iff the bit width is strictly greater. E.g.
                // i16 is a superset of u8, but i16 is not a superset of u16.
                ((Signed, us), (Unsigned, lower_bound)) => us > lower_bound,
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FloatWidth {
    Dec,
    F32,
    F64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignDemand {
    /// Can be signed or unsigned.
    NoDemand,
    /// Must be signed.
    Signed,
}

/// Describes a bound on the width of an integer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IntBound {
    /// There is no bound on the width.
    None,
    /// Must have an exact width.
    Exact(IntLitWidth),
    /// Must have a certain sign and a minimum width.
    AtLeast {
        sign: SignDemand,
        width: IntLitWidth,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FloatBound {
    None,
    Exact(FloatWidth),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NumBound {
    None,
    /// Must be an integer of a certain size, or any float.
    AtLeastIntOrFloat {
        sign: SignDemand,
        width: IntLitWidth,
    },
}

pub const fn int_lit_width_to_variable(w: IntLitWidth) -> Variable {
    match w {
        IntLitWidth::U8 => Variable::U8,
        IntLitWidth::U16 => Variable::U16,
        IntLitWidth::U32 => Variable::U32,
        IntLitWidth::U64 => Variable::U64,
        IntLitWidth::U128 => Variable::U128,
        IntLitWidth::I8 => Variable::I8,
        IntLitWidth::I16 => Variable::I16,
        IntLitWidth::I32 => Variable::I32,
        IntLitWidth::I64 => Variable::I64,
        IntLitWidth::I128 => Variable::I128,
        IntLitWidth::Nat => Variable::NAT,
        IntLitWidth::F32 => Variable::F32,
        IntLitWidth::F64 => Variable::F64,
        IntLitWidth::Dec => Variable::DEC,
    }
}

pub const fn float_width_to_variable(w: FloatWidth) -> Variable {
    match w {
        FloatWidth::Dec => Variable::DEC,
        FloatWidth::F32 => Variable::F32,
        FloatWidth::F64 => Variable::F64,
    }
}

const ALL_VARIABLES: &[Variable] = &[
    Variable::I8,
    Variable::U8,
    Variable::I16,
    Variable::U16,
    Variable::F32,
    Variable::I32,
    Variable::U32,
    Variable::F64,
    Variable::I64,
    Variable::NAT, // FIXME: Nat's order here depends on the platform
    Variable::U64,
    Variable::I128,
    Variable::DEC,
    Variable::U128,
];

const SIGNED_VARIABLES: &[Variable] = &[
    Variable::I8,
    Variable::I16,
    Variable::F32,
    Variable::I32,
    Variable::F64,
    Variable::I64,
    Variable::I128,
    Variable::DEC,
];
