use crate::{log::Log, state::State};

pub trait Backend {
    type Tx: Transaction;

    fn begin_tx(&mut self) -> Result<Self::Tx, Error>;
}

#[derive(Debug, thiserror::Error)]
pub enum Error {}

pub trait Transaction: Log + State {
    fn commit(self) -> Result<(), Error>;
}
