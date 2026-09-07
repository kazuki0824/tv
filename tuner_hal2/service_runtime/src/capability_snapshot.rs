use std::collections::BTreeMap;

use crate::capability_selection::{
    select_capabilities, CapabilityCandidate, CapabilityClaims, CapabilityClosure,
    CapabilityClosureProfile, CapabilityResource, ProductCapabilityProfile, SelectedCapabilities,
};
use crate::playback_consume_txn::required_playback_processing_bytes;
use maleicacid_tuner_hal2_common::{HalError, HalInternalKind, HalInvalidArgumentKind};
use maleicacid_tuner_hal2_demux::{
    DvrKind, FilterOpenType, DEFAULT_AV_MAX_EVENT_BYTES,
    DEFAULT_AV_MAX_OUTSTANDING_EVENTS_PER_FILTER, DEFAULT_AV_PER_FILTER_LIVE_BYTES,
    MAX_PES_BUFFER_BYTES,
};

const MIB: usize = 1024 * 1024;
const DEMUX_FILTER_MAIN_TYPE_TS: i32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PublicDemuxCapability {
    pub id: i32,
    pub filter_types: i32,
}

impl PublicDemuxCapability {
    pub const fn ts(id: i32) -> Self {
        Self {
            id,
            filter_types: DEMUX_FILTER_MAIN_TYPE_TS,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapabilitySnapshot {
    pub public_demuxes: [Option<PublicDemuxCapability>; 8],
    pub num_record: i32,
    pub num_playback: i32,
    pub num_ts_filter: i32,
    pub num_section_filter: i32,
    pub num_audio_filter: i32,
    pub num_video_filter: i32,
    pub num_pes_filter: i32,
    pub num_pcr_filter: i32,
    pub filter_pending_event_capacity_per_filter: usize,
    pub fmq_runtime_budget_bytes: usize,
    pub pes_max_bytes_per_filter: usize,
    pub pes_runtime_budget_bytes: usize,
    pub playback_processing_budget_bytes: usize,
    pub av_max_event_bytes: usize,
    pub av_max_outstanding_events_per_filter: usize,
    pub av_per_filter_live_bytes: usize,
    pub av_runtime_budget_bytes: usize,
    pub cleanup_reaper_capacity: usize,
    pub cleanup_retry_schedule_ms: [u64; 4],
    pub cleanup_terminal_deadline_ms: u64,
    pub worker_io_deadline_ms: u64,
    pub worker_reaper_deadline_ms: u64,
}

impl CapabilitySnapshot {
    pub const fn product_default() -> Self {
        Self {
            public_demuxes: [
                Some(PublicDemuxCapability::ts(1)),
                Some(PublicDemuxCapability::ts(2)),
                Some(PublicDemuxCapability::ts(3)),
                Some(PublicDemuxCapability::ts(4)),
                Some(PublicDemuxCapability::ts(5)),
                Some(PublicDemuxCapability::ts(6)),
                Some(PublicDemuxCapability::ts(7)),
                Some(PublicDemuxCapability::ts(8)),
            ],
            num_record: 8,
            num_playback: 8,
            num_ts_filter: 32,
            num_section_filter: 8,
            // TS audio は producer 側の frame/sample 関連付けにより、PTS header が
            // 疎な PES でも event-associated timestamp を確定できる。
            num_audio_filter: 1,
            // 製品対象の MPEG-2 / AVC / HEVC video PES は ARIB profile 上 PTS を明示する。
            num_video_filter: 1,
            num_pes_filter: 4,
            num_pcr_filter: 4,
            filter_pending_event_capacity_per_filter: 64,
            fmq_runtime_budget_bytes: 256 * MIB,
            pes_max_bytes_per_filter: MAX_PES_BUFFER_BYTES,
            // TsPesだけでなくTsAudio/TsVideoもper-filter PesAssemblerを持つため、
            // 4 PES + 1 AUDIO + 1 VIDEOの全6本を同時に閉じる。
            pes_runtime_budget_bytes: 6 * MIB,
            playback_processing_budget_bytes: 64 * MIB,
            av_max_event_bytes: DEFAULT_AV_MAX_EVENT_BYTES,
            av_max_outstanding_events_per_filter: DEFAULT_AV_MAX_OUTSTANDING_EVENTS_PER_FILTER,
            av_per_filter_live_bytes: DEFAULT_AV_PER_FILTER_LIVE_BYTES,
            av_runtime_budget_bytes: DEFAULT_AV_PER_FILTER_LIVE_BYTES * 2,
            cleanup_reaper_capacity: 160,
            cleanup_retry_schedule_ms: [0, 10, 100, 1_000],
            cleanup_terminal_deadline_ms: 30_000,
            worker_io_deadline_ms: 2_000,
            worker_reaper_deadline_ms: 10_000,
        }
    }

    pub(crate) fn compose_for_frontends(
        frontend_ids: &[i32],
    ) -> Result<(Self, Vec<i32>), HalError> {
        use CapabilityClosure::*;
        use CapabilityResource::*;
        let requested = Self::product_default();
        // 製品宣言の共有枠とbyte予算。個別APIで要求される実領域は起動時に先取りしない。
        let available = CapabilityClaims::default()
            .with(
                Worker,
                requested
                    .cleanup_reaper_capacity
                    .checked_mul(3)
                    .ok_or_else(|| {
                        HalError::internal(
                            HalInternalKind::InvariantViolation,
                            "worker上限がoverflowしました",
                        )
                    })?,
            )
            .with(Callback, requested.cleanup_reaper_capacity)
            .with(
                Reaper,
                frontend_ids
                    .len()
                    .checked_mul(2)
                    .and_then(|count| requested.cleanup_reaper_capacity.checked_add(count))
                    .ok_or_else(|| {
                        HalError::internal(
                            HalInternalKind::InvariantViolation,
                            "reaper上限がoverflowしました",
                        )
                    })?,
            )
            .with(Cleanup, requested.cleanup_reaper_capacity)
            .with(SectionTracker, requested.num_section_filter as usize)
            .with(FmqBytes, requested.fmq_runtime_budget_bytes)
            .with(PesBytes, requested.pes_runtime_budget_bytes)
            .with(AvBytes, requested.av_runtime_budget_bytes)
            .with(PlaybackBytes, requested.playback_processing_budget_bytes);
        let object = CapabilityClaims::default()
            .with(Worker, 3)
            .with(Callback, 1)
            .with(Reaper, 2)
            .with(Cleanup, 2);
        let mut closures = Vec::new();
        let mut ids = frontend_ids.to_vec();
        ids.sort_unstable();
        if ids.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "能力候補のfrontend IDが重複しています",
            ));
        }
        for id in &ids {
            closures.push(Self::closure_profile(
                Frontend(*id),
                1,
                object.with(Worker, 6).with(Reaper, 4),
                None,
            )?);
        }
        closures.push(Self::closure_profile(
            DemuxBase,
            requested.public_demuxes().map(|entries| entries.len())?,
            object,
            None,
        )?);
        // TS/SECTION/PCR/PES/AV/playback/recordの順で共有枠を使用する。
        // 非公開候補は暗黙の0件であり、正の候補を大きい順に1件刻みで試す。
        for (closure, count, unit, count_limit) in [
            (
                TsFilter,
                requested.num_ts_filter,
                object.with(FmqBytes, 4 * MIB),
                None,
            ),
            (
                SectionFilter,
                requested.num_section_filter,
                object.with(FmqBytes, 4 * MIB).with(SectionTracker, 1),
                None,
            ),
            (PcrFilter, requested.num_pcr_filter, object, None),
            (
                Pes,
                requested.num_pes_filter,
                object
                    .with(FmqBytes, 8 * MIB)
                    .with(PesBytes, requested.pes_max_bytes_per_filter),
                Some((DemuxBase, 1)),
            ),
            (
                AudioAv,
                requested.num_audio_filter,
                object
                    .with(PesBytes, requested.pes_max_bytes_per_filter)
                    .with(AvBytes, requested.av_per_filter_live_bytes),
                None,
            ),
            (
                VideoAv,
                requested.num_video_filter,
                object
                    .with(PesBytes, requested.pes_max_bytes_per_filter)
                    .with(AvBytes, requested.av_per_filter_live_bytes),
                None,
            ),
            (
                PlaybackDvr,
                requested.num_playback,
                object.with(FmqBytes, 4 * MIB).with(PlaybackBytes, 8 * MIB),
                Some((DemuxBase, 1)),
            ),
            (
                RecordDvr,
                requested.num_record,
                object.with(FmqBytes, 4 * MIB),
                Some((DemuxBase, 1)),
            ),
        ] {
            let count = usize::try_from(count).map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "能力候補の個数が負数です",
                )
            })?;
            closures.push(Self::closure_profile(closure, count, unit, count_limit)?);
        }
        let profile = ProductCapabilityProfile {
            shared_runtime_candidates: vec![available],
            closures,
        };
        let selected = select_capabilities(&profile, available, |selection| {
            let snapshot = Self::from_selection(requested, selection)
                .map_err(|_| "能力候補の変換が不正です")?;
            snapshot
                .validate_dependency_closures()
                .map_err(|_| "能力閉包の横断検査に失敗しました")?;
            let filter_count = [TsFilter, SectionFilter, PcrFilter, Pes, AudioAv, VideoAv]
                .iter()
                .try_fold(0usize, |sum, kind| sum.checked_add(selection.count(*kind)))
                .ok_or("filter個数がoverflowしました")?;
            let frontend_count = ids
                .iter()
                .filter(|id| selection.count(Frontend(**id)) > 0)
                .count();
            let object_count = filter_count
                .checked_add(frontend_count)
                .and_then(|count| count.checked_add(selection.count(DemuxBase)))
                .and_then(|count| count.checked_add(selection.count(PlaybackDvr)))
                .and_then(|count| count.checked_add(selection.count(RecordDvr)))
                .ok_or("object個数がoverflowしました")?;
            let claims = selection.claims();
            if claims.amount(Callback) < object_count
                || claims.amount(Worker)
                    < object_count
                        .checked_add(frontend_count)
                        .and_then(|count| count.checked_add(claims.amount(Reaper)))
                        .ok_or("worker個数がoverflowしました")?
                || claims.amount(Reaper)
                    < object_count
                        .checked_mul(2)
                        .ok_or("reaper個数がoverflowしました")?
                || claims.amount(Cleanup)
                    < object_count
                        .checked_mul(2)
                        .ok_or("cleanup個数がoverflowしました")?
                || claims.amount(SectionTracker) != selection.count(SectionFilter)
            {
                return Err("共有枠またはSECTION追跡領域が不足しています");
            }
            Ok(())
        })
        .map_err(|error| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                format!("{} 返却順={:?}", error.reason, error.returned_in_order),
            )
        })?;
        let snapshot = Self::from_selection(requested, &selected)?;
        let ids = ids
            .into_iter()
            .filter(|id| selected.count(Frontend(*id)) > 0)
            .collect();
        Ok((snapshot, ids))
    }

    fn closure_profile(
        closure: CapabilityClosure,
        maximum: usize,
        unit: CapabilityClaims,
        count_limit: Option<(CapabilityClosure, usize)>,
    ) -> Result<CapabilityClosureProfile, HalError> {
        let dependencies = match closure {
            CapabilityClosure::Frontend(_) | CapabilityClosure::DemuxBase => Vec::new(),
            _ => vec![CapabilityClosure::DemuxBase],
        };
        let candidates = (1..=maximum)
            .rev()
            .map(|count| {
                let claims = unit.checked_scale(count).ok_or_else(|| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "能力候補の資源量がoverflowしました",
                    )
                })?;
                Ok(CapabilityCandidate { count, claims })
            })
            .collect::<Result<Vec<_>, HalError>>()?;
        Ok(CapabilityClosureProfile {
            closure,
            dependencies,
            candidates,
            count_limit,
        })
    }

    fn from_selection(
        mut snapshot: Self,
        selected: &SelectedCapabilities,
    ) -> Result<Self, HalError> {
        use CapabilityClosure::*;
        use CapabilityResource::*;
        let count = |closure| {
            i32::try_from(selected.count(closure)).map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "公開能力の個数が範囲外です",
                )
            })
        };
        snapshot.num_ts_filter = count(TsFilter)?;
        snapshot.num_section_filter = count(SectionFilter)?;
        snapshot.num_pcr_filter = count(PcrFilter)?;
        snapshot.num_pes_filter = count(Pes)?;
        snapshot.num_audio_filter = count(AudioAv)?;
        snapshot.num_video_filter = count(VideoAv)?;
        snapshot.num_playback = count(PlaybackDvr)?;
        snapshot.num_record = count(RecordDvr)?;
        let has_filters = [TsFilter, SectionFilter, PcrFilter, Pes, AudioAv, VideoAv]
            .iter()
            .any(|kind| selected.count(*kind) > 0);
        for (index, entry) in snapshot.public_demuxes.iter_mut().enumerate() {
            if index >= selected.count(DemuxBase) {
                *entry = None;
            } else if let Some(entry) = entry {
                entry.filter_types = if has_filters {
                    DEMUX_FILTER_MAIN_TYPE_TS
                } else {
                    0
                };
            }
        }
        let claims = selected.claims();
        snapshot.fmq_runtime_budget_bytes = claims.amount(FmqBytes);
        snapshot.pes_runtime_budget_bytes = claims.amount(PesBytes);
        snapshot.av_runtime_budget_bytes = claims.amount(AvBytes);
        snapshot.playback_processing_budget_bytes = claims.amount(PlaybackBytes);
        snapshot.cleanup_reaper_capacity = selected
            .frontend_count()
            .checked_mul(2)
            .and_then(|frontend_reapers| claims.amount(Reaper).checked_sub(frontend_reapers))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend reaper分離が不整合です",
                )
            })?;
        if !has_filters {
            snapshot.filter_pending_event_capacity_per_filter = 0;
        }
        if snapshot.pes_runtime_budget_bytes == 0 {
            snapshot.pes_max_bytes_per_filter = 0;
        }
        if snapshot.av_runtime_budget_bytes == 0 {
            snapshot.av_max_event_bytes = 0;
            snapshot.av_max_outstanding_events_per_filter = 0;
            snapshot.av_per_filter_live_bytes = 0;
        }
        Ok(snapshot)
    }

    pub const fn filter_capacity(self, open_type: FilterOpenType) -> i32 {
        match open_type {
            FilterOpenType::TsUndefined | FilterOpenType::TsRaw | FilterOpenType::TsRecord => {
                self.num_ts_filter
            }
            FilterOpenType::TsSection => self.num_section_filter,
            FilterOpenType::TsAudio => self.num_audio_filter,
            FilterOpenType::TsVideo => self.num_video_filter,
            FilterOpenType::TsPes => self.num_pes_filter,
            FilterOpenType::TsPcr => self.num_pcr_filter,
        }
    }

    pub fn public_demuxes(&self) -> Result<Vec<PublicDemuxCapability>, HalError> {
        let first_empty = match self.public_demuxes.iter().position(Option::is_none) {
            Some(index) => index,
            None => self.public_demuxes.len(),
        };
        if self.public_demuxes[first_empty..]
            .iter()
            .any(Option::is_some)
        {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "published demux capability map contains a hole",
            ));
        }
        Ok(self.public_demuxes[..first_empty]
            .iter()
            .filter_map(|entry| *entry)
            .collect())
    }

    pub fn public_demux_ids(&self) -> Result<Vec<i32>, HalError> {
        self.public_demuxes().map(|entries| {
            entries
                .into_iter()
                .map(|entry| entry.id)
                .collect::<Vec<_>>()
        })
    }

    pub fn public_demux_filter_types(&self, demux_id: i32) -> Result<Option<i32>, HalError> {
        self.public_demuxes().map(|entries| {
            entries
                .into_iter()
                .find(|entry| entry.id == demux_id)
                .map(|entry| entry.filter_types)
        })
    }

    pub fn public_demux_filter_caps(&self) -> Result<i32, HalError> {
        self.public_demuxes().map(|entries| {
            entries
                .into_iter()
                .fold(0, |caps, entry| caps | entry.filter_types)
        })
    }

    pub fn validate_dependency_closures(self) -> Result<(), HalError> {
        let public_demuxes = self.public_demuxes()?;
        if public_demuxes.iter().any(|entry| {
            entry.id < 0
                || entry.filter_types < 0
                || (entry.filter_types & !DEMUX_FILTER_MAIN_TYPE_TS) != 0
        }) || public_demuxes
            .windows(2)
            .any(|entries| entries[0].id >= entries[1].id)
        {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                concat!(
                    "published demux capabilities must have sorted unique ids and supported ",
                    "filter bits",
                ),
            ));
        }
        let has_published_ts_filter = [
            self.num_ts_filter,
            self.num_section_filter,
            self.num_audio_filter,
            self.num_video_filter,
            self.num_pes_filter,
            self.num_pcr_filter,
        ]
        .into_iter()
        .any(|count| count > 0);
        let has_demux_dependent_capability =
            has_published_ts_filter || self.num_record > 0 || self.num_playback > 0;
        if public_demuxes.is_empty() && has_demux_dependent_capability {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "filter or DVR capability cannot be published without a demux",
            ));
        }
        if public_demuxes.iter().any(|entry| {
            ((entry.filter_types & DEMUX_FILTER_MAIN_TYPE_TS) != 0) != has_published_ts_filter
        }) {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "published demux filter types disagree with the filter capability closure",
            ));
        }
        let published_filter_count = [
            self.num_ts_filter,
            self.num_section_filter,
            self.num_audio_filter,
            self.num_video_filter,
            self.num_pes_filter,
            self.num_pcr_filter,
        ]
        .into_iter()
        .try_fold(0usize, |total, count| {
            usize::try_from(count)
                .ok()
                .and_then(|count| total.checked_add(count))
        })
        .ok_or_else(|| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "published filter capability count is invalid",
            )
        })?;
        if published_filter_count == 0 {
            if self.filter_pending_event_capacity_per_filter != 0 {
                return Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "suppressed filter capability must not reserve pending event entries",
                ));
            }
        } else if self.filter_pending_event_capacity_per_filter == 0
            || self
                .filter_pending_event_capacity_per_filter
                .checked_mul(published_filter_count)
                .is_none()
        {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "pending filter event capability closure is not finite",
            ));
        }
        let av_filter_count = usize::try_from(
            self.num_audio_filter
                .checked_add(self.num_video_filter)
                .ok_or_else(|| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "AV filter capability count overflow",
                    )
                })?,
        )
        .map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "AV filter capability count must not be negative",
            )
        })?;
        if av_filter_count == 0 {
            if self.av_max_event_bytes != 0
                || self.av_max_outstanding_events_per_filter != 0
                || self.av_per_filter_live_bytes != 0
                || self.av_runtime_budget_bytes != 0
            {
                return Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "suppressed AV capability must not reserve or advertise AV byte budgets",
                ));
            }
        } else {
            if self.av_max_event_bytes == 0
                || self.av_max_outstanding_events_per_filter == 0
                || self.av_per_filter_live_bytes == 0
                || self.av_runtime_budget_bytes == 0
                || self.av_max_event_bytes > MAX_PES_BUFFER_BYTES
            {
                return Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "advertised AV capability exceeds its finite PES assembly closure",
                ));
            }
            let minimum_per_filter = self
                .av_max_event_bytes
                .checked_mul(self.av_max_outstanding_events_per_filter)
                .ok_or_else(|| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "AV per-filter capability closure overflow",
                    )
                })?;
            if self.av_per_filter_live_bytes < minimum_per_filter {
                return Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "AV per-filter budget does not close the advertised event capability",
                ));
            }
            let minimum_runtime = self
                .av_per_filter_live_bytes
                .checked_mul(av_filter_count)
                .ok_or_else(|| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "AV runtime capability closure overflow",
                    )
                })?;
            if self.av_runtime_budget_bytes < minimum_runtime {
                return Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "AV runtime budget does not close all advertised AV filter leases",
                ));
            }
        }
        let pes_assembler_filter_count = self
            .num_pes_filter
            .checked_add(self.num_audio_filter)
            .and_then(|count| count.checked_add(self.num_video_filter))
            .and_then(|count| usize::try_from(count).ok())
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "PES assembler filter capability count is invalid",
                )
            })?;
        if pes_assembler_filter_count == 0 {
            if self.pes_max_bytes_per_filter != 0 || self.pes_runtime_budget_bytes != 0 {
                return Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "suppressed PES assembler capability must not reserve PES byte budgets",
                ));
            }
        } else {
            let minimum_pes_runtime = self
                .pes_max_bytes_per_filter
                .checked_mul(pes_assembler_filter_count)
                .ok_or_else(|| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "PES runtime capability closure overflow",
                    )
                })?;
            if self.pes_max_bytes_per_filter == 0
                || self.pes_runtime_budget_bytes < minimum_pes_runtime
            {
                return Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "PES runtime budget does not close all advertised PES/AV assembler leases",
                ));
            }
        }
        let published_object_count = [
            i32::try_from(public_demuxes.len()).map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "published demux capability count overflow",
                )
            })?,
            self.num_record,
            self.num_playback,
            self.num_ts_filter,
            self.num_section_filter,
            self.num_audio_filter,
            self.num_video_filter,
            self.num_pes_filter,
            self.num_pcr_filter,
        ]
        .into_iter()
        .try_fold(0usize, |total, count| {
            usize::try_from(count)
                .ok()
                .and_then(|count| total.checked_add(count))
        })
        .ok_or_else(|| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "published object capability count is invalid",
            )
        })?;
        let minimum_reaper_capacity = published_object_count.checked_mul(2).ok_or_else(|| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "cleanup reaper capability closure overflow",
            )
        })?;
        if self.cleanup_reaper_capacity < minimum_reaper_capacity
            || self.cleanup_retry_schedule_ms != [0, 10, 100, 1_000]
            || self.cleanup_terminal_deadline_ms != 30_000
            || self.worker_io_deadline_ms != 2_000
            || self.worker_reaper_deadline_ms != 10_000
        {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "cleanup capability closure is not finite for all published objects",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ByteClaim {
    fmq: usize,
    pes: usize,
    playback_processing: usize,
}

#[derive(Debug, Default)]
pub(crate) struct CapacityLedger {
    filter_claims: BTreeMap<i32, ByteClaim>,
    dvr_claims: BTreeMap<i32, ByteClaim>,
    fmq_used: usize,
    pes_used: usize,
    playback_processing_used: usize,
}

impl CapacityLedger {
    fn reserve_total(used: usize, amount: usize, limit: usize) -> Result<usize, HalError> {
        let next = used.checked_add(amount).ok_or_else(|| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "capacity ledger byte counter overflow",
            )
        })?;
        if next > limit {
            return Err(HalError::unsupported_detail(
                "capacity ledger",
                "capability snapshot byte budget is exhausted",
            ));
        }
        Ok(next)
    }

    fn request_bytes(buffer_size: i32, resource: &'static str) -> Result<usize, HalError> {
        usize::try_from(buffer_size)
            .ok()
            .filter(|size| *size > 0)
            .ok_or_else(|| {
                HalError::invalid_argument(
                    HalInvalidArgumentKind::NumericRange,
                    format!("{resource} buffer size must be positive"),
                )
            })
    }

    pub(crate) fn reserve_filter(
        &mut self,
        snapshot: CapabilitySnapshot,
        filter_id: i32,
        open_type: FilterOpenType,
        buffer_size: i32,
    ) -> Result<(), HalError> {
        if self.filter_claims.contains_key(&filter_id) {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "filter capacity claim already exists",
            ));
        }
        let requested_buffer_size = Self::request_bytes(buffer_size, "filter")?;
        let fmq = if open_type.has_filter_fmq() {
            requested_buffer_size
        } else {
            0
        };
        let pes = if matches!(
            open_type,
            FilterOpenType::TsPes | FilterOpenType::TsAudio | FilterOpenType::TsVideo
        ) {
            snapshot.pes_max_bytes_per_filter
        } else {
            0
        };
        let next_fmq = Self::reserve_total(self.fmq_used, fmq, snapshot.fmq_runtime_budget_bytes)?;
        let next_pes = Self::reserve_total(self.pes_used, pes, snapshot.pes_runtime_budget_bytes)?;
        self.filter_claims.insert(
            filter_id,
            ByteClaim {
                fmq,
                pes,
                playback_processing: 0,
            },
        );
        self.fmq_used = next_fmq;
        self.pes_used = next_pes;
        Ok(())
    }

    pub(crate) fn release_filter(&mut self, filter_id: i32) -> Result<(), HalError> {
        let claim = self.filter_claims.get(&filter_id).copied().ok_or_else(|| {
            HalError::cleanup_failed(
                "filter capacity release",
                "filter capacity claim is missing",
            )
        })?;
        let next_fmq = self.fmq_used.checked_sub(claim.fmq).ok_or_else(|| {
            HalError::internal(HalInternalKind::InvariantViolation, "FMQ ledger underflow")
        })?;
        let next_pes = self.pes_used.checked_sub(claim.pes).ok_or_else(|| {
            HalError::internal(HalInternalKind::InvariantViolation, "PES ledger underflow")
        })?;
        self.filter_claims.remove(&filter_id);
        self.fmq_used = next_fmq;
        self.pes_used = next_pes;
        Ok(())
    }

    pub(crate) fn reserve_dvr(
        &mut self,
        snapshot: CapabilitySnapshot,
        dvr_id: i32,
        buffer_size: i32,
    ) -> Result<(), HalError> {
        if self.dvr_claims.contains_key(&dvr_id) {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "DVR capacity claim already exists",
            ));
        }
        let fmq = Self::request_bytes(buffer_size, "DVR")?;
        let next_fmq = Self::reserve_total(self.fmq_used, fmq, snapshot.fmq_runtime_budget_bytes)?;
        self.dvr_claims.insert(
            dvr_id,
            ByteClaim {
                fmq,
                pes: 0,
                playback_processing: 0,
            },
        );
        self.fmq_used = next_fmq;
        Ok(())
    }

    pub(crate) fn reserve_playback_processing(
        &mut self,
        snapshot: CapabilitySnapshot,
        dvr_id: i32,
        kind: DvrKind,
        buffer_size: i32,
    ) -> Result<bool, HalError> {
        if kind != DvrKind::Playback {
            return Ok(false);
        }
        let queue_capacity = Self::request_bytes(buffer_size, "playback processing")?;
        let amount = required_playback_processing_bytes(queue_capacity);
        let claim = self.dvr_claims.get_mut(&dvr_id).ok_or_else(|| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "DVR capacity claim is missing during configure",
            )
        })?;
        if claim.playback_processing != 0 {
            return Ok(false);
        }
        let next = self
            .playback_processing_used
            .checked_add(amount)
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "playback processing capacity counter overflow",
                )
            })?;
        if next > snapshot.playback_processing_budget_bytes {
            return Err(HalError::out_of_memory(
                "playback processing budget",
                "playback processing capacity is exhausted",
            ));
        }
        claim.playback_processing = amount;
        self.playback_processing_used = next;
        Ok(true)
    }

    pub(crate) fn rollback_playback_processing(&mut self, dvr_id: i32) -> Result<(), HalError> {
        let claim = self.dvr_claims.get_mut(&dvr_id).ok_or_else(|| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "DVR capacity claim is missing during configure rollback",
            )
        })?;
        let amount = std::mem::take(&mut claim.playback_processing);
        self.playback_processing_used = self
            .playback_processing_used
            .checked_sub(amount)
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "playback processing ledger underflow",
                )
            })?;
        Ok(())
    }

    pub(crate) fn release_dvr(&mut self, dvr_id: i32) -> Result<(), HalError> {
        let claim = self.dvr_claims.remove(&dvr_id).ok_or_else(|| {
            HalError::cleanup_failed("DVR capacity release", "DVR capacity claim is missing")
        })?;
        self.fmq_used = self.fmq_used.checked_sub(claim.fmq).ok_or_else(|| {
            HalError::internal(HalInternalKind::InvariantViolation, "FMQ ledger underflow")
        })?;
        self.playback_processing_used = self
            .playback_processing_used
            .checked_sub(claim.playback_processing)
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "playback processing ledger underflow",
                )
            })?;
        Ok(())
    }

    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_cleanup_shortage_preserves_unrelated_capabilities() {
        let (snapshot, ids) =
            CapabilitySnapshot::compose_for_frontends(&[1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        assert_eq!(ids, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(
            (snapshot.num_ts_filter, snapshot.num_section_filter),
            (32, 8)
        );
        assert_eq!(
            (
                snapshot.num_audio_filter,
                snapshot.num_video_filter,
                snapshot.num_pes_filter
            ),
            (1, 1, 4)
        );
        assert_eq!((snapshot.num_playback, snapshot.num_record), (8, 6));
        assert_eq!(snapshot.cleanup_reaper_capacity, 160);
        snapshot.validate_dependency_closures().unwrap();
    }

    #[test]
    fn absent_frontend_does_not_suppress_demux_and_dvr() {
        let (snapshot, ids) = CapabilitySnapshot::compose_for_frontends(&[]).unwrap();
        assert!(ids.is_empty());
        assert_eq!((snapshot.num_playback, snapshot.num_record), (8, 8));
        assert_eq!(snapshot.fmq_runtime_budget_bytes, 256 * MIB);
        snapshot.validate_dependency_closures().unwrap();
    }

    #[test]
    fn demux_dependent_capability_requires_a_published_demux() {
        let mut snapshot = CapabilitySnapshot::product_default();
        snapshot.public_demuxes = [None; 8];
        assert!(snapshot.validate_dependency_closures().is_err());
    }

    #[test]
    fn suppressed_pes_assembler_capability_requires_zero_pes_budgets() {
        let mut snapshot = CapabilitySnapshot::product_default();
        snapshot.num_pes_filter = 0;
        snapshot.num_audio_filter = 0;
        snapshot.num_video_filter = 0;
        snapshot.av_max_event_bytes = 0;
        snapshot.av_max_outstanding_events_per_filter = 0;
        snapshot.av_per_filter_live_bytes = 0;
        snapshot.av_runtime_budget_bytes = 0;
        assert!(snapshot.validate_dependency_closures().is_err());

        snapshot.pes_max_bytes_per_filter = 0;
        snapshot.pes_runtime_budget_bytes = 0;
        assert!(snapshot.validate_dependency_closures().is_ok());
    }

    #[test]
    fn pending_event_capacity_tracks_published_filter_capability() {
        let mut snapshot = CapabilitySnapshot::product_default();
        snapshot.filter_pending_event_capacity_per_filter = 0;
        assert!(snapshot.validate_dependency_closures().is_err());

        snapshot.num_ts_filter = 0;
        snapshot.num_section_filter = 0;
        snapshot.num_audio_filter = 0;
        snapshot.num_video_filter = 0;
        snapshot.num_pes_filter = 0;
        snapshot.num_pcr_filter = 0;
        snapshot.pes_max_bytes_per_filter = 0;
        snapshot.pes_runtime_budget_bytes = 0;
        snapshot.av_max_event_bytes = 0;
        snapshot.av_max_outstanding_events_per_filter = 0;
        snapshot.av_per_filter_live_bytes = 0;
        snapshot.av_runtime_budget_bytes = 0;
        snapshot.num_record = 0;
        snapshot.num_playback = 0;
        snapshot.public_demuxes = [None; 8];
        assert!(snapshot.validate_dependency_closures().is_ok());
    }

    #[test]
    fn failed_reservation_does_not_consume_budget() {
        let snapshot = CapabilitySnapshot {
            fmq_runtime_budget_bytes: 1024,
            ..CapabilitySnapshot::product_default()
        };
        let mut ledger = CapacityLedger::default();
        assert!(ledger
            .reserve_filter(snapshot, 1, FilterOpenType::TsRaw, 2048)
            .is_err());
        ledger
            .reserve_filter(snapshot, 1, FilterOpenType::TsRaw, 1024)
            .expect("failed reservation must leave budget unchanged");
        ledger.release_filter(1).expect("release succeeds");
    }

    #[test]
    fn record_filter_claims_filter_fmq_bytes_while_payloadless_filter_does_not() {
        let snapshot = CapabilitySnapshot {
            fmq_runtime_budget_bytes: 4096,
            ..CapabilitySnapshot::product_default()
        };
        let mut ledger = CapacityLedger::default();
        ledger
            .reserve_filter(snapshot, 1, FilterOpenType::TsPcr, 4096)
            .unwrap();
        ledger.release_filter(1).unwrap();
        ledger
            .reserve_filter(snapshot, 2, FilterOpenType::TsRecord, 4096)
            .unwrap();
        assert!(ledger
            .reserve_filter(snapshot, 3, FilterOpenType::TsRaw, 1)
            .is_err());
        ledger.release_filter(2).unwrap();
        ledger
            .reserve_filter(snapshot, 3, FilterOpenType::TsRaw, 4096)
            .unwrap();
        ledger.release_filter(3).unwrap();
    }

    #[test]
    fn playback_processing_is_reserved_once_and_released_with_dvr() {
        let snapshot = CapabilitySnapshot::product_default();
        let mut ledger = CapacityLedger::default();
        ledger.reserve_dvr(snapshot, 7, 4096).unwrap();
        ledger
            .reserve_playback_processing(snapshot, 7, DvrKind::Playback, 4096)
            .unwrap();
        ledger
            .reserve_playback_processing(snapshot, 7, DvrKind::Playback, 4096)
            .unwrap();
        ledger.release_dvr(7).unwrap();
    }

    #[test]
    fn playback_processing_claim_matches_bounded_allocation() {
        let queue_capacity = 4 * MIB;
        let processing_bytes = required_playback_processing_bytes(queue_capacity);
        let snapshot = CapabilitySnapshot {
            playback_processing_budget_bytes: processing_bytes,
            ..CapabilitySnapshot::product_default()
        };
        let mut ledger = CapacityLedger::default();
        ledger
            .reserve_dvr(snapshot, 7, queue_capacity as i32)
            .unwrap();
        ledger
            .reserve_dvr(snapshot, 8, queue_capacity as i32)
            .unwrap();
        assert!(ledger
            .reserve_playback_processing(snapshot, 7, DvrKind::Playback, queue_capacity as i32)
            .is_ok());
        assert!(ledger
            .reserve_playback_processing(snapshot, 8, DvrKind::Playback, queue_capacity as i32)
            .is_err());
        ledger.release_dvr(7).unwrap();
        assert!(ledger
            .reserve_playback_processing(snapshot, 8, DvrKind::Playback, queue_capacity as i32)
            .is_ok());
        ledger.release_dvr(8).unwrap();
    }

    #[test]
    fn av_filters_claim_their_pes_assembly_budget() {
        let snapshot = CapabilitySnapshot {
            pes_runtime_budget_bytes: MAX_PES_BUFFER_BYTES,
            ..CapabilitySnapshot::product_default()
        };
        let mut ledger = CapacityLedger::default();
        ledger
            .reserve_filter(snapshot, 1, FilterOpenType::TsAudio, 4096)
            .unwrap();
        assert!(ledger
            .reserve_filter(snapshot, 2, FilterOpenType::TsVideo, 4096)
            .is_err());
        ledger.release_filter(1).unwrap();
        ledger
            .reserve_filter(snapshot, 2, FilterOpenType::TsVideo, 4096)
            .unwrap();
        ledger.release_filter(2).unwrap();
    }

    #[test]
    fn av_filter_open_does_not_preclaim_payload_budget() {
        let snapshot = CapabilitySnapshot {
            num_audio_filter: 0,
            num_video_filter: 1,
            av_max_event_bytes: DEFAULT_AV_MAX_EVENT_BYTES,
            av_max_outstanding_events_per_filter: DEFAULT_AV_MAX_OUTSTANDING_EVENTS_PER_FILTER,
            av_per_filter_live_bytes: DEFAULT_AV_PER_FILTER_LIVE_BYTES,
            av_runtime_budget_bytes: 1,
            ..CapabilitySnapshot::product_default()
        };
        let mut ledger = CapacityLedger::default();
        ledger
            .reserve_filter(snapshot, 1, FilterOpenType::TsVideo, 4096)
            .unwrap();
        ledger.release_filter(1).unwrap();
        ledger
            .reserve_filter(snapshot, 2, FilterOpenType::TsVideo, 4096)
            .unwrap();
        ledger.release_filter(2).unwrap();
    }

    #[test]
    fn product_snapshot_closes_audio_and_video_av_dependencies() {
        let snapshot = CapabilitySnapshot::product_default();
        assert_eq!(snapshot.num_audio_filter, 1);
        assert_eq!(snapshot.num_video_filter, 1);
        assert_eq!(snapshot.num_pes_filter, 4);
        assert!(snapshot.pes_runtime_budget_bytes >= snapshot.pes_max_bytes_per_filter * 6);
        snapshot
            .validate_dependency_closures()
            .expect("product AV capabilities must retain a closed finite byte budget");
    }
}
