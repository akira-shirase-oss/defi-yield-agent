//! The recorded adapter: a scripted list of answers, the last repeated. Tests and the load
//! harness only (feature `test-support`).

use std::collections::VecDeque;
use std::sync::Mutex;

use crate::port::{FetchError, FetchFuture, PoolSource};

#[derive(Debug, Clone)]
pub enum Step {
    Body(Vec<u8>),
    Fail(FetchError),
    /// Never answers: the caller's deadline must end it.
    Hang,
}

#[derive(Debug)]
pub struct Recorded {
    name: &'static str,
    steps: Mutex<VecDeque<Step>>,
}

impl Recorded {
    pub fn new(name: &'static str, steps: impl IntoIterator<Item = Step>) -> Self {
        Self {
            name,
            steps: Mutex::new(steps.into_iter().collect()),
        }
    }

    /// Replaces the script (tests that change the source between rounds).
    pub fn script(&self, steps: impl IntoIterator<Item = Step>) {
        let mut guard = self.steps.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = steps.into_iter().collect();
    }

    fn next(&self) -> Step {
        let mut guard = self.steps.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if guard.len() > 1 {
            guard.pop_front().unwrap_or(Step::Fail(FetchError::Connect))
        } else {
            guard.front().cloned().unwrap_or(Step::Fail(FetchError::Connect))
        }
    }
}

impl PoolSource for Recorded {
    fn name(&self) -> &'static str {
        self.name
    }

    fn fetch(&self) -> FetchFuture<'_> {
        let step = self.next();
        Box::pin(async move {
            match step {
                Step::Body(b) => Ok(b),
                Step::Fail(e) => Err(e),
                Step::Hang => std::future::pending().await,
            }
        })
    }
}
