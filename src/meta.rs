use crate::{log::Key, site::SiteId, space::SpaceId};

#[derive(Debug)]
pub struct Meta {
    id: SpaceId,
    site_id: SiteId,
    /// The key of the last entry applied to the state.
    /// This is never below the base key.
    watermark: Option<Key>,
    /// True means that log holds at least one entry with key older than
    /// `watermark` that the state does not reflect, or the base was replaced.
    /// The state is then stale, and only a full replay fixes it.
    dirty: bool,
}

impl Meta {
    pub fn site(&self) -> SiteId {
        self.site_id
    }

    pub fn watermark(&self) -> Option<&Key> {
        self.watermark.as_ref()
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
}

pub trait Store {
    fn get(&self) -> Result<Meta, Error>;
    fn set_watermark(&mut self, watermark: Key) -> Result<(), Error>;
    fn set_dirty(&mut self, dirty: bool) -> Result<(), Error>;
}

#[derive(Debug, thiserror::Error)]
pub enum Error {}
