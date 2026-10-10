use crate::{hlc::HybridLogicalClock, site::SiteId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Key {
    hlc: HybridLogicalClock,
    site: SiteId,
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use super::*;

    #[test]
    fn key_order_with_hlc() {
        let hlc1 = HybridLogicalClock::new::<SystemTime>();
        let hlc2 = hlc1.now::<SystemTime>();
        let site = SiteId::new();

        let key1 = Key { hlc: hlc1, site };
        let key2 = Key { hlc: hlc2, site };

        assert!(key2 > key1);
    }

    #[test]
    fn key_order_with_hlc_tie() {
        let hlc = HybridLogicalClock::new::<SystemTime>();
        let site1 = SiteId::new();
        let site2 = SiteId::new();
        let (larger_site, smaller_site) = if site1 < site2 {
            (site2, site1)
        } else {
            (site1, site2)
        };

        let key1 = Key {
            hlc,
            site: larger_site,
        };
        let key2 = Key {
            hlc,
            site: smaller_site,
        };

        assert!(key1 > key2);
    }

    #[test]
    fn key_equality() {
        let hlc = HybridLogicalClock::new::<SystemTime>();
        let site = SiteId::new();

        let key1 = Key { hlc, site };
        let key2 = Key { hlc, site };

        assert_eq!(key1, key2);
    }
}
