//! Tuner HAL 内部の同期原語を集約する骨格。
//!
//! r50dz25 WP-03 では production 経路の lock 取得補助として使う。
//! mutex 汚染、lock 失敗、wait 失敗、既定値丸めを共通の失敗分類へ寄せる。

use std::sync::{Condvar, Mutex, MutexGuard};

use binder::{Status, StatusCode};
use maleicacid_tuner_hal_common::HalError;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum HalLockError {
    Poisoned,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum HalWaitError {
    Poisoned,
    Timeout,
}


pub fn poisoned_lock_status(name: &'static str) -> Status {
    eprintln!("maleicacid-tuner-hal: mutex poison fail-closed: {name}");
    Status::from(StatusCode::UNKNOWN_ERROR)
}

pub fn lock_mutex_status<'a, T>(
    mutex: &'a Mutex<T>,
    name: &'static str,
) -> binder::Result<MutexGuard<'a, T>> {
    mutex.lock().map_err(|_| poisoned_lock_status(name))
}


pub fn lock_mutex_hal<'a, T>(
    mutex: &'a Mutex<T>,
    name: &'static str,
) -> Result<MutexGuard<'a, T>, HalError> {
    mutex
        .lock()
        .map_err(|_| HalError::Internal(format!("poisoned mutex fail-closed: {name}")))
}

pub fn lock_mutex_io<'a, T>(
    mutex: &'a Mutex<T>,
    name: &'static str,
) -> std::io::Result<MutexGuard<'a, T>> {
    mutex.lock().map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("poisoned mutex fail-closed: {name}"),
        )
    })
}

pub fn lock_mutex_option<'a, T>(mutex: &'a Mutex<T>, name: &'static str) -> Option<MutexGuard<'a, T>> {
    match mutex.lock() {
        Ok(guard) => Some(guard),
        Err(_) => {
            eprintln!("maleicacid-tuner-hal: mutex poison fail-closed: {name}");
            None
        }
    }
}

pub struct HalMutex<T> {
    inner: Mutex<T>,
}

impl<T> HalMutex<T> {
    pub fn new(value: T) -> Self {
        Self { inner: Mutex::new(value) }
    }

    pub fn lock(&self) -> Result<HalMutexGuard<'_, T>, HalLockError> {
        self.inner
            .lock()
            .map(HalMutexGuard)
            .map_err(|_| HalLockError::Poisoned)
    }
}

pub struct HalMutexGuard<'a, T>(MutexGuard<'a, T>);

impl<'a, T> std::ops::Deref for HalMutexGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a, T> std::ops::DerefMut for HalMutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub struct HalCondvar {
    inner: Condvar,
}

impl HalCondvar {
    pub fn new() -> Self {
        Self { inner: Condvar::new() }
    }

    pub fn notify_all(&self) {
        self.inner.notify_all();
    }

    pub fn wait<'a, T>(
        &self,
        guard: HalMutexGuard<'a, T>,
    ) -> Result<HalMutexGuard<'a, T>, HalWaitError> {
        self.inner
            .wait(guard.0)
            .map(HalMutexGuard)
            .map_err(|_| HalWaitError::Poisoned)
    }
}

impl Default for HalCondvar {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hal_mutex_reports_poison_instead_of_recovering_inner() {
        let mutex = std::sync::Arc::new(HalMutex::new(1_i32));
        let cloned = std::sync::Arc::clone(&mutex);
        let _ = std::thread::spawn(move || {
            let _guard = cloned.lock().expect("initial lock must succeed");
            panic!("poison test");
        })
        .join();

        assert_eq!(mutex.lock().err(), Some(HalLockError::Poisoned));
    }
}
