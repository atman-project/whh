pub trait State {
    type Operation;
    type Snapshot;

    fn apply(&mut self, op: Self::Operation) -> Result<ApplyOutcome, Error>;
    fn reset(&mut self) -> Result<(), Error>;
    fn export(&mut self) -> Result<Self::Snapshot, Error>;
    fn import(&mut self, snapshot: &Self::Snapshot) -> Result<(), Error>;
}

#[derive(Debug, thiserror::Error)]
pub enum Error {}

pub enum ApplyOutcome {
    Applied,
    Rejected(String),
}
