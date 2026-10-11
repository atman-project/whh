pub use crate::log::key::Key;
use crate::{hlc::HybridLogicalClock, site::SiteId, version::VersionVector};

mod key;

pub trait Log: ReadOnlyLog {
    fn append(&mut self, key: Key, op: Self::Operation) -> Result<(), Error>;
    fn delete_through(&mut self, key: &Key) -> Result<(), Error>;
}

pub trait ReadOnlyLog {
    type Operation;

    fn get(&self, key: &Key) -> Option<&Self::Operation>;
    fn scan_after(
        &self,
        after: Option<&Key>,
    ) -> Result<impl Iterator<Item = &Entry<Self::Operation>>, Error>;
    fn scan_site_after(
        &self,
        site: &SiteId,
        after: Option<&Key>,
    ) -> Result<impl Iterator<Item = &Entry<Self::Operation>>, Error>;
    fn version_vector(&self) -> Result<VersionVector, Error>;
    fn max_hlc(&self) -> Result<Option<HybridLogicalClock>, Error>;
}

pub struct Entry<Operation> {
    pub key: Key,
    pub op: Operation,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {}
