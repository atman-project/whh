use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SiteId(Uuid);

impl SiteId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}
