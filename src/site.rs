use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SiteId([u8; 16]);

impl SiteId {
    pub fn random() -> Self {
        Self(Uuid::new_v4().into_bytes())
    }
}
