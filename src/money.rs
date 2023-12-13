use std::ops::{Mul, Sub};

use crate::Error;

// TODO: consider using rusty-money crate
#[derive(Clone, Copy)]
pub(crate) struct MoneyUnit(i64);

impl MoneyUnit {
    pub(crate) const fn from_cents(v: i64) -> Self {
        Self(v)
    }

    pub(crate) const fn to_cents_as_i64(self) -> i64 {
        self.0
    }
}

// The multiplication of MoneyUnit is checked by default
// because it's very important to avoid overflows or underflows
impl Mul<i64> for MoneyUnit {
    type Output = Option<Self>;

    fn mul(self, rhs: i64) -> Self::Output {
        self.0.checked_mul(rhs).map(Self)
    }
}

impl Mul<i32> for MoneyUnit {
    type Output = Option<Self>;

    fn mul(self, rhs: i32) -> Self::Output {
        self.0.checked_mul(rhs as i64).map(Self)
    }
}

// MoneyUnit does not implement SubAssign because
// AddSub cannot return an option.
//
// ```
// fn sub_assign(&mut self, rhs: Rhs);
// ```
impl Sub<Self> for MoneyUnit {
    type Output = Result<Self, Error>;

    fn sub(self, rhs: Self) -> Self::Output {
        if self.0 < 0 {
            return Err(Error::NegativeBalance);
        };
        let res = self
            .0
            .checked_sub(rhs.0)
            .ok_or(Error::BalanceWouldUnderflow)?;
        if res < 0 {
            return Err(Error::BalanceWouldBecomeNegative);
        }
        Ok(Self(res))
    }
}
