use crate::{
    base::{self, Base},
    log::{self},
    meta::{self, Meta},
    space::SpaceId,
    state,
};

pub trait Backend: Sized {
    type Tx: Transaction;
    type Log: log::ReadOnlyLog;

    fn open(space: SpaceId) -> Result<Self, Error>;
    fn begin_tx(&mut self) -> Result<Self::Tx, Error>;
    fn meta(&self) -> Result<Option<Meta>, Error>;
    fn base(&self) -> Result<Option<Base>, Error>;
    fn log(&self) -> &Self::Log;
}

#[derive(Debug, thiserror::Error)]
pub enum Error {}

pub trait Transaction {
    type Meta: meta::Store;
    type Base: base::Store;
    type Log: log::Log;
    type State: state::State;

    fn meta(&mut self) -> &mut Self::Meta;
    fn base(&mut self) -> &mut Self::Base;
    fn log(&mut self) -> &mut Self::Log;
    fn state(&mut self) -> &mut Self::State;
    fn commit(self) -> Result<(), Error>;
}
