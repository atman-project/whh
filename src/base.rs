use crate::{hlc::HybridLogicalClock, log::Key, site::SiteId};

#[derive(Debug)]
pub struct Base {
    key: Key,
    created_by: SiteId,
    state: StateSnapshot,
}

impl Base {
    pub fn hlc(&self) -> HybridLogicalClock {
        self.key.hlc()
    }
}

// TODO: define
type StateSnapshot = ();

pub trait Store {
    fn get(&self) -> Result<Option<Base>, Error>;
    fn set(&mut self, base: Base) -> Result<(), Error>;
}

#[derive(Debug, thiserror::Error)]
pub enum Error {}
