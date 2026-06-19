use maleicacid_tuner_hal2_common::HalError;
use crate::{AidlApi, AidlObjectKind, CommandPlan};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DescramblerCommand {
    SetDemuxSource(i32),
    SetKeyToken(Vec<u8>),
    AddPid(u16),
    RemovePid(u16),
    Close,
}

impl DescramblerCommand {
    pub fn plan(&self) -> Result<CommandPlan, HalError> {
        match self {
            Self::SetDemuxSource(_) => CommandPlan::for_api(AidlObjectKind::Descrambler, AidlApi::DescramblerSetDemuxSource),
            Self::SetKeyToken(_) => CommandPlan::for_api(AidlObjectKind::Descrambler, AidlApi::DescramblerSetKeyToken),
            Self::AddPid(_) => CommandPlan::for_api(AidlObjectKind::Descrambler, AidlApi::DescramblerAddPid),
            Self::RemovePid(_) => CommandPlan::for_api(AidlObjectKind::Descrambler, AidlApi::DescramblerRemovePid),
            Self::Close => CommandPlan::for_api(AidlObjectKind::Descrambler, AidlApi::DescramblerClose),
        }
    }
}
