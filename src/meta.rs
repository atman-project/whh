use crate::{log::Key, site::SiteId, space::SpaceId};

#[derive(Debug)]
pub struct Meta {
    id: SpaceId,
    site_id: SiteId,
    watermark: Option<Key>,
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
