use crate::{log::key::Key, site::SiteId, version::VersionVector};

mod key;

pub trait Log {
    type Operation;

    fn append(&mut self, op: Self::Operation);
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
    fn delete_through(&mut self, key: &Key) -> Result<(), Error>;
    fn version_vector(&self) -> Result<VersionVector, Error>;
}

pub struct Entry<Operation> {
    pub key: Key,
    pub op: Operation,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {}
