use crate::{hlc::HybridLogicalClock, log::Key, site::SiteId};

#[derive(Debug)]
pub struct Base<Snapshot> {
    key: Key,
    created_by: SiteId,
    state: Snapshot,
}

impl<Snapshot> Base<Snapshot> {
    pub fn key(&self) -> &Key {
        &self.key
    }

    pub fn hlc(&self) -> HybridLogicalClock {
        self.key.hlc()
    }

    pub fn state(&self) -> &Snapshot {
        &self.state
    }
}

pub trait Store {
    type Snapshot;

    fn get(&self) -> Result<Option<Base<Self::Snapshot>>, Error>;
    fn set(&mut self, base: Base<Self::Snapshot>) -> Result<(), Error>;
}

#[derive(Debug, thiserror::Error)]
pub enum Error {}
