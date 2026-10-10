use std::{
    cmp::max,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct HybridLogicalClock(u64);

impl HybridLogicalClock {
    pub fn new<C: Clock>() -> Self {
        Self::pack(C::now_millis(), 0)
    }

    fn pack(ms: u64, counter: u64) -> Self {
        Self((ms << 16) | counter)
    }

    pub fn now<C: Clock>(self) -> Self {
        max(Self::new::<C>(), Self(self.0 + 1))
    }

    pub fn observe(self, other: Self) -> Self {
        max(self, other)
    }
}

pub trait Clock {
    fn now_millis() -> u64;
}

impl Clock for SystemTime {
    fn now_millis() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("current time must be after UNIX_EPOCH")
            .as_millis()
            .try_into()
            .expect("millis time must fit into u64")
    }
}

#[cfg(test)]
mod tests {
    use std::{
        thread::sleep,
        time::{Duration, SystemTime},
    };

    use super::*;

    #[test]
    fn cmp() {
        let a = HybridLogicalClock::new::<SystemTime>();
        sleep(Duration::from_millis(1));
        let b = HybridLogicalClock::new::<SystemTime>();
        assert!(b > a);
    }

    #[test]
    fn now() {
        let a0 = HybridLogicalClock::new::<SystemTime>();
        let a1 = a0.now::<SystemTime>();
        assert!(a1 > a0);
    }

    #[test]
    fn observe() {
        let a0 = HybridLogicalClock::new::<SystemTime>();
        sleep(Duration::from_millis(1));
        let b0 = HybridLogicalClock::new::<SystemTime>();

        assert_eq!(a0.observe(b0), b0);
        assert_eq!(b0.observe(a0), b0);
    }
}
