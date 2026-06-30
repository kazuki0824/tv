use maleicacid_tuner_hal2_common::HalError;
use maleicacid_tuner_hal2_domain_request::{AidlObjectGeneration, AidlObjectId, AidlObjectKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ObjectDomainCleanupKind {
    Frontend,
    Lnb,
    LnbDropLeakRecord,
}

impl ObjectDomainCleanupKind {
    pub(crate) fn close_for_object_kind(object_kind: AidlObjectKind) -> Option<Self> {
        match object_kind {
            AidlObjectKind::Frontend => Some(Self::Frontend),
            AidlObjectKind::Lnb => Some(Self::Lnb),
            AidlObjectKind::Tuner
            | AidlObjectKind::Demux
            | AidlObjectKind::Filter
            | AidlObjectKind::Dvr
            | AidlObjectKind::Descrambler => None,
        }
    }

    pub(crate) fn drop_leak_for_object_kind(object_kind: AidlObjectKind) -> Option<Self> {
        match object_kind {
            AidlObjectKind::Frontend => Some(Self::Frontend),
            AidlObjectKind::Lnb => Some(Self::LnbDropLeakRecord),
            AidlObjectKind::Tuner
            | AidlObjectKind::Demux
            | AidlObjectKind::Filter
            | AidlObjectKind::Dvr
            | AidlObjectKind::Descrambler => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectDomainCleanupCommand {
    object_kind: AidlObjectKind,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    cleanup_kind: ObjectDomainCleanupKind,
}

impl ObjectDomainCleanupCommand {
    pub(crate) fn new(
        object_kind: AidlObjectKind,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        cleanup_kind: ObjectDomainCleanupKind,
    ) -> Self {
        Self {
            object_kind,
            object_id,
            generation,
            cleanup_kind,
        }
    }

    pub fn object_kind(&self) -> AidlObjectKind {
        self.object_kind
    }

    pub fn object_id(&self) -> AidlObjectId {
        self.object_id
    }

    pub fn generation(&self) -> AidlObjectGeneration {
        self.generation
    }

    pub(crate) fn execute_with<E>(self, executor: &mut E) -> ObjectDomainCleanupOutcome
    where
        E: ObjectDomainCleanupExecutor,
    {
        let result = match self.cleanup_kind {
            ObjectDomainCleanupKind::Frontend => executor.execute_frontend_cleanup(self),
            ObjectDomainCleanupKind::Lnb => executor.execute_lnb_cleanup(self),
            ObjectDomainCleanupKind::LnbDropLeakRecord => executor.execute_lnb_drop_leak_record(self),
        };
        ObjectDomainCleanupOutcome::completed(self, result)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ObjectDomainCleanupOutcome {
    result: Result<(), HalError>,
}

impl ObjectDomainCleanupOutcome {
    pub(crate) fn completed(
        _command: ObjectDomainCleanupCommand,
        result: Result<(), HalError>,
    ) -> Self {
        Self { result }
    }

    pub(crate) fn result(&self) -> Result<(), HalError> {
        self.result.clone()
    }
}

pub trait ObjectDomainCleanupExecutor {
    fn execute_frontend_cleanup(
        &mut self,
        command: ObjectDomainCleanupCommand,
    ) -> Result<(), HalError>;

    fn execute_lnb_cleanup(
        &mut self,
        command: ObjectDomainCleanupCommand,
    ) -> Result<(), HalError>;

    fn execute_lnb_drop_leak_record(
        &mut self,
        command: ObjectDomainCleanupCommand,
    ) -> Result<(), HalError>;
}
