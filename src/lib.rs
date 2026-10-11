use std::marker::PhantomData;

use crate::{
    backend::Transaction,
    hlc::HybridLogicalClock,
    log::{Key, Log, ReadOnlyLog},
    meta::Store,
    state::State,
};

mod backend;
mod base;
mod hlc;
mod log;
mod meta;
mod site;
mod space;
mod state;
mod version;

pub struct Whh<Backend, Clock, Operation> {
    backend: Backend,
    hlc: HybridLogicalClock,
    _phantom: PhantomData<(Clock, Operation)>,
}

impl<Backend, Clock, Operation> Whh<Backend, Clock, Operation>
where
    Backend: backend::Backend,
    <Backend::Tx as Transaction>::Log: Log<Operation = Operation>,
    <Backend::Tx as Transaction>::State: State<Operation = Operation>,
    Clock: hlc::Clock,
    Operation: Clone,
{
    pub fn open(backend: Backend) -> Result<Self, Error> {
        let hlc = Self::load_hlc(&backend)?;
        Ok(Self {
            backend,
            hlc,
            _phantom: PhantomData,
        })
    }

    fn load_hlc(backend: &Backend) -> Result<HybridLogicalClock, Error> {
        let mut hlc = HybridLogicalClock::new::<Clock>();

        if let Some(max_hlc_in_log) = backend.log().max_hlc()? {
            hlc = hlc.observe(max_hlc_in_log)
        }

        if let Some(base) = backend.base()? {
            hlc = hlc.observe(base.hlc())
        }

        Ok(hlc)
    }

    pub fn write(&mut self, op: Operation) -> Result<Key, Error> {
        let mut tx = self.backend.begin_tx()?;

        self.settle()?;

        // always > watermark, since self.hlc > every observed HLC
        self.hlc = self.hlc.now::<Clock>();

        let meta = tx.meta().get()?;
        let key = Key::new(self.hlc, meta.site());

        match tx.state().apply(op.clone())? {
            state::ApplyOutcome::Applied => {
                tx.log().append(key, op)?;
                tx.meta().set_watermark(key)?;
                tx.commit()?;
                Ok(key)
            }
            state::ApplyOutcome::Rejected(reason) => Err(Error::OperationRejected(reason)),
        }
    }

    pub fn settle(&mut self) -> Result<(), Error> {
        todo!()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Backend(#[from] backend::Error),
    #[error(transparent)]
    Log(#[from] log::Error),
    #[error(transparent)]
    State(#[from] state::Error),
    #[error(transparent)]
    Meta(#[from] meta::Error),
    #[error(transparent)]
    Base(#[from] base::Error),
    #[error("operation rejected: {0}")]
    OperationRejected(String),
}
