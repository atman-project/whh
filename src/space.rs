use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpaceId([u8; 16]);

impl SpaceId {
    pub fn random() -> Self {
        Self(Uuid::new_v4().into_bytes())
    }
}
