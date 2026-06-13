use crate::{AidlApi, AidlObjectKind, CommandPlan, RuntimeTransactionName};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DescramblerCommand {
    SetDemuxSource(i32),
    SetKeyToken(Vec<u8>),
    AddPid(u16),
    RemovePid(u16),
    Close,
}

impl DescramblerCommand {
    pub fn plan(&self) -> CommandPlan {
        match self {
            Self::SetDemuxSource(_) => CommandPlan { object: AidlObjectKind::Descrambler, api: AidlApi::DescramblerSetDemuxSource, transaction: RuntimeTransactionName::DescramblerSessionTxnSetDemuxSource },
            Self::SetKeyToken(_) => CommandPlan { object: AidlObjectKind::Descrambler, api: AidlApi::DescramblerSetKeyToken, transaction: RuntimeTransactionName::DescramblerSessionTxnSetKeyToken },
            Self::AddPid(_) => CommandPlan { object: AidlObjectKind::Descrambler, api: AidlApi::DescramblerAddPid, transaction: RuntimeTransactionName::DescramblerSessionTxnAddPid },
            Self::RemovePid(_) => CommandPlan { object: AidlObjectKind::Descrambler, api: AidlApi::DescramblerRemovePid, transaction: RuntimeTransactionName::DescramblerSessionTxnRemovePid },
            Self::Close => CommandPlan { object: AidlObjectKind::Descrambler, api: AidlApi::DescramblerClose, transaction: RuntimeTransactionName::DescramblerSessionTxnClose },
        }
    }
}
