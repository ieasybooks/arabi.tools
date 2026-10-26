use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use anyhow::{Context, ensure};
use tashkeel::{Diacritizer, InferenceOptions, Model, Progress, models};
use tokio::sync::Semaphore;

use super::text::CHUNK_SCALARS;

pub(super) trait Engine: Send {
    fn diacritize(
        &mut self,
        text: &str,
        progress: &mut dyn FnMut(Progress) -> ControlFlow<()>,
    ) -> tashkeel::Result<String>;
}

impl Engine for Diacritizer {
    fn diacritize(
        &mut self,
        text: &str,
        progress: &mut dyn FnMut(Progress) -> ControlFlow<()>,
    ) -> tashkeel::Result<String> {
        self.diacritize_with_progress(text, progress)
    }
}

type LoadEngine = dyn Fn(Model) -> anyhow::Result<Box<dyn Engine>> + Send + Sync;

pub(super) struct EnginePool {
    pub permits: Arc<Semaphore>,
    load: Box<LoadEngine>,
    state: Mutex<State>,
    limit: usize,
}

#[derive(Default)]
struct State {
    idle: Vec<Entry>,
    /// Includes engines currently borrowed or being loaded.
    count: usize,
}

struct Entry {
    model: Model,
    engine: Box<dyn Engine>,
}

impl EnginePool {
    pub fn load(
        cache_dir: PathBuf,
        runtime_path: Option<PathBuf>,
        limit: usize,
    ) -> anyhow::Result<Self> {
        ensure!(limit >= 2, "diacritization needs room for both models");
        let pool = Self::new(
            limit,
            Box::new(move |model| {
                let directory = models::model_dir(model, &cache_dir);
                models::verify(model, &directory).with_context(|| format!("verifying {model}"))?;
                let mut engine = Diacritizer::load(
                    model,
                    &directory,
                    InferenceOptions {
                        threads: 1,
                        chunk_size: CHUNK_SCALARS,
                        runtime_path: runtime_path.clone(),
                    },
                )
                .with_context(|| format!("loading {model}"))?;
                // A real prediction checks runtime/model compatibility before accepting jobs.
                engine
                    .diacritize("مرحبا")
                    .with_context(|| format!("probing {model}"))?;
                Ok(Box::new(engine))
            }),
        );
        drop(pool.borrow(Model::Shakkelha)?);
        drop(pool.borrow(Model::Catt)?);
        Ok(pool)
    }

    pub fn new(limit: usize, load: Box<LoadEngine>) -> Self {
        Self {
            permits: Arc::new(Semaphore::new(limit)),
            load,
            state: Mutex::default(),
            limit,
        }
    }

    /// Called only on blocking workers, holding a pool permit (or during startup).
    pub fn borrow(&self, model: Model) -> anyhow::Result<Lease<'_>> {
        let replaced = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(index) = state.idle.iter().position(|entry| entry.model == model) {
                return Ok(Lease {
                    pool: self,
                    entry: Some(state.idle.swap_remove(index)),
                });
            }
            if state.count < self.limit {
                state.count += 1;
                None
            } else {
                Some(
                    state
                        .idle
                        .pop()
                        .context("all diacritization engines are occupied")?,
                )
            }
        };
        // An empty lease rolls back the count if loading fails or panics. Dispose of an
        // idle engine before its replacement is loaded, keeping the total within limit.
        let mut lease = Lease {
            pool: self,
            entry: None,
        };
        drop(replaced);
        let engine = (self.load)(model)?;
        lease.entry = Some(Entry { model, engine });
        Ok(lease)
    }
}

pub(super) struct Lease<'a> {
    pool: &'a EnginePool,
    entry: Option<Entry>,
}

impl Lease<'_> {
    pub fn engine(&mut self) -> &mut dyn Engine {
        self.entry.as_mut().expect("loaded engine").engine.as_mut()
    }
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        let mut state = self
            .pool
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(entry) = self.entry.take() {
            state.idle.push(entry);
        } else {
            state.count -= 1;
        }
    }
}
