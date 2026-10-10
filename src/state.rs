pub trait State {
    type Operation;

    fn apply(&mut self, op: Self::Operation) -> Result<ApplyOutcome, Error>;
    fn reset(&mut self) -> Result<(), Error>;
    fn export(&mut self) -> Result<Vec<u8>, Error>;
    fn import(&mut self, state: &[u8]) -> Result<(), Error>;
}

#[derive(Debug, thiserror::Error)]
pub enum Error {}

pub enum ApplyOutcome {
    Applied,
    Rejected(String),
}
