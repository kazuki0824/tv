use std::sync::{Arc, Mutex};

use binder::{FromIBinder, Interface, Strong};

#[derive(Clone)]
pub struct AidlCallbackSlot<T: Interface + FromIBinder + ?Sized> {
    inner: Arc<Mutex<Option<Strong<T>>>>,
}

impl<T: Interface + FromIBinder + ?Sized> Default for AidlCallbackSlot<T> {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
        }
    }
}

impl<T: Interface + FromIBinder + ?Sized> AidlCallbackSlot<T> {
    pub fn retain(&self, callback: &Strong<T>) -> Result<(), AidlCallbackSlotError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| AidlCallbackSlotError::Poisoned)?;
        *inner = Some(callback.clone());
        Ok(())
    }

    pub fn clear(&self) -> Result<(), AidlCallbackSlotError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| AidlCallbackSlotError::Poisoned)?;
        *inner = None;
        Ok(())
    }

    pub fn is_registered(&self) -> Result<bool, AidlCallbackSlotError> {
        let inner = self
            .inner
            .lock()
            .map_err(|_| AidlCallbackSlotError::Poisoned)?;
        Ok(inner.is_some())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AidlCallbackSlotError {
    Poisoned,
}
