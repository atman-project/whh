use std::marker::PhantomData;

use crate::{
    backend::Transaction,
    base::Store as _,
    hlc::HybridLogicalClock,
    log::{Entry, Key, Log, ReadOnlyLog},
    meta::Store as _,
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
    Backend::Log: ReadOnlyLog<Operation = Operation>,
    <Backend::Tx as Transaction>::Log: Log<Operation = Operation>,
    <Backend::Tx as Transaction>::State: State<Operation = Operation, Snapshot = Backend::Snapshot>,
    <Backend::Tx as Transaction>::Base: base::Store<Snapshot = Backend::Snapshot>,
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

    /// Local write
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
        if self.backend.meta()?.expect("meta must exist").is_dirty() {
            self.replay()
        } else {
            self.apply_new_logs()
        }
    }

    fn replay(&mut self) -> Result<(), Error> {
        let mut tx = self.backend.begin_tx()?;
        let mut new_watermark = match tx.base().get()? {
            Some(base) => {
                tx.state().import(base.state())?;
                Some(*base.key())
            }
            None => {
                tx.state().reset()?;
                None
            }
        };

        // TODO: clear `failed`

        for entry in self.backend.log().scan_after(None)? {
            Self::apply_log_entry(&mut tx, entry)?;
            new_watermark = Some(*entry.key());
        }

        let mut meta = tx.meta();
        if let Some(new_watermark) = new_watermark {
            meta.set_watermark(new_watermark)?;
        }
        meta.set_dirty(false)?;

        Ok(tx.commit()?)
    }

    fn apply_new_logs(&mut self) -> Result<(), Error> {
        let mut tx = self.backend.begin_tx()?;
        let meta = tx.meta().get()?;
        let mut new_watermark: Option<Key> = None;
        for entry in self.backend.log().scan_after(meta.watermark())? {
            Self::apply_log_entry(&mut tx, entry)?;
            new_watermark = Some(*entry.key())
        }

        if let Some(new_watermark) = new_watermark {
            tx.meta().set_watermark(new_watermark)?;
        }
        Ok(tx.commit()?)
    }

    fn apply_log_entry(tx: &mut Backend::Tx, entry: &Entry<Operation>) -> Result<(), Error> {
        match tx.state().apply(entry.operation().clone())? {
            state::ApplyOutcome::Applied => Ok(()),
            state::ApplyOutcome::Rejected(reason) => {
                // TODO: insert to `failed`
                Err(Error::OperationRejected(reason))
            }
        }
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
