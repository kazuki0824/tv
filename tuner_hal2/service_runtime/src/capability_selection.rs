use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum CapabilityClosure {
    Frontend(i32),
    DemuxBase,
    TsFilter,
    SectionFilter,
    PcrFilter,
    Pes,
    AudioAv,
    VideoAv,
    PlaybackDvr,
    RecordDvr,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub enum CapabilityResource {
    Worker,
    Callback,
    Reaper,
    Cleanup,
    SectionTracker,
    FmqBytes,
    PesBytes,
    AvBytes,
    PlaybackBytes,
}

const RESOURCE_COUNT: usize = 9;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CapabilityClaims([usize; RESOURCE_COUNT]);

impl CapabilityClaims {
    pub fn with(mut self, resource: CapabilityResource, amount: usize) -> Self {
        self.0[resource as usize] = amount;
        self
    }

    pub fn amount(self, resource: CapabilityResource) -> usize {
        self.0[resource as usize]
    }

    pub fn checked_scale(self, count: usize) -> Option<Self> {
        let mut next = Self::default();
        for (result, amount) in next.0.iter_mut().zip(self.0) {
            *result = amount.checked_mul(count)?;
        }
        Some(next)
    }

    fn checked_add(self, other: Self) -> Option<Self> {
        let mut next = Self::default();
        for (index, result) in next.0.iter_mut().enumerate() {
            *result = self.0[index].checked_add(other.0[index])?;
        }
        Some(next)
    }

    fn checked_sub(self, other: Self) -> Option<Self> {
        let mut next = Self::default();
        for (index, result) in next.0.iter_mut().enumerate() {
            *result = self.0[index].checked_sub(other.0[index])?;
        }
        Some(next)
    }

    fn fits(self, limit: Self) -> bool {
        self.0
            .iter()
            .zip(limit.0)
            .all(|(used, maximum)| *used <= maximum)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapabilityCandidate {
    pub count: usize,
    pub claims: CapabilityClaims,
}

#[derive(Clone, Debug)]
pub struct CapabilityClosureProfile {
    pub closure: CapabilityClosure,
    pub dependencies: Vec<CapabilityClosure>,
    // 配列順が製品の固定優先順。候補間で資源量を継ぎ合わせない。
    pub candidates: Vec<CapabilityCandidate>,
    pub count_limit: Option<(CapabilityClosure, usize)>,
}

#[derive(Clone, Debug)]
pub struct ProductCapabilityProfile {
    pub shared_runtime_candidates: Vec<CapabilityClaims>,
    pub closures: Vec<CapabilityClosureProfile>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectedCapabilities {
    selected: BTreeMap<CapabilityClosure, CapabilityCandidate>,
    claims: CapabilityClaims,
}

impl SelectedCapabilities {
    pub fn count(&self, closure: CapabilityClosure) -> usize {
        self.selected
            .get(&closure)
            .map_or(0, |candidate| candidate.count)
    }

    pub fn frontend_count(&self) -> usize {
        self.selected
            .iter()
            .filter(|(closure, candidate)| {
                matches!(closure, CapabilityClosure::Frontend(_)) && candidate.count > 0
            })
            .count()
    }

    pub fn claims(&self) -> CapabilityClaims {
        self.claims
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilitySelectionError {
    pub reason: &'static str,
    pub returned_in_order: Vec<CapabilityClosure>,
}

// 仮予約の唯一の所有者。選択後の横断検証が成功するまでcommitしない。
struct CapabilityReservationTxn {
    limit: CapabilityClaims,
    used: CapabilityClaims,
    reserved: Vec<(CapabilityClosure, CapabilityCandidate)>,
}

impl CapabilityReservationTxn {
    fn reserve(&mut self, closure: CapabilityClosure, candidate: CapabilityCandidate) -> bool {
        let Some(next) = self.used.checked_add(candidate.claims) else {
            return false;
        };
        if !next.fits(self.limit) {
            return false;
        }
        self.reserved.push((closure, candidate));
        self.used = next;
        true
    }

    fn rollback(mut self, reason: &'static str) -> CapabilitySelectionError {
        let mut returned_in_order = Vec::new();
        while let Some((closure, candidate)) = self.reserved.pop() {
            let Some(next) = self.used.checked_sub(candidate.claims) else {
                return CapabilitySelectionError {
                    reason: "能力仮予約の返却量が不整合です",
                    returned_in_order,
                };
            };
            self.used = next;
            returned_in_order.push(closure);
        }
        CapabilitySelectionError {
            reason,
            returned_in_order,
        }
    }
}

pub fn select_capabilities(
    profile: &ProductCapabilityProfile,
    available: CapabilityClaims,
    validate: impl FnOnce(&SelectedCapabilities) -> Result<(), &'static str>,
) -> Result<SelectedCapabilities, CapabilitySelectionError> {
    let Some(limit) = profile
        .shared_runtime_candidates
        .iter()
        .copied()
        .find(|limit| limit.fits(available))
    else {
        return Err(CapabilitySelectionError {
            reason: "共有runtime候補を予約できません",
            returned_in_order: Vec::new(),
        });
    };
    let mut reservation = CapabilityReservationTxn {
        limit,
        used: CapabilityClaims::default(),
        reserved: Vec::new(),
    };
    let mut selected = BTreeMap::new();
    for closure in &profile.closures {
        if selected.contains_key(&closure.closure)
            || closure
                .dependencies
                .iter()
                .any(|dependency| !selected.contains_key(dependency))
        {
            return Err(reservation.rollback("能力候補の重複または依存順序が不正です"));
        }
        if closure.dependencies.iter().any(|dependency| {
            selected
                .get(dependency)
                .map_or(true, |candidate: &CapabilityCandidate| candidate.count == 0)
        }) {
            selected.insert(
                closure.closure,
                CapabilityCandidate {
                    count: 0,
                    claims: CapabilityClaims::default(),
                },
            );
            continue;
        }
        let mut accepted = CapabilityCandidate {
            count: 0,
            claims: CapabilityClaims::default(),
        };
        for candidate in &closure.candidates {
            if let Some((dependency, per_owner)) = closure.count_limit {
                let Some(limit) = selected
                    .get(&dependency)
                    .and_then(|owner| owner.count.checked_mul(per_owner))
                else {
                    return Err(reservation.rollback("能力の所有者別上限が不正です"));
                };
                if candidate.count > limit {
                    continue;
                }
            }
            if candidate.count == 0 && candidate.claims != CapabilityClaims::default() {
                return Err(reservation.rollback("非公開の能力候補に資源claimがあります"));
            }
            if reservation.reserve(closure.closure, *candidate) {
                accepted = *candidate;
                break;
            }
        }
        selected.insert(closure.closure, accepted);
    }
    let selected = SelectedCapabilities {
        selected,
        claims: reservation.used,
    };
    if let Err(reason) = validate(&selected) {
        return Err(reservation.rollback(reason));
    }
    Ok(selected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use CapabilityClosure::{AudioAv, DemuxBase, PlaybackDvr, SectionFilter, TsFilter};
    use CapabilityResource::{Callback, FmqBytes, Reaper, SectionTracker, Worker};

    fn claims(workers: usize, bytes: usize) -> CapabilityClaims {
        CapabilityClaims::default()
            .with(Worker, workers)
            .with(FmqBytes, bytes)
    }

    fn closure(
        closure: CapabilityClosure,
        dependencies: &[CapabilityClosure],
        candidates: &[(usize, CapabilityClaims)],
    ) -> CapabilityClosureProfile {
        CapabilityClosureProfile {
            closure,
            dependencies: dependencies.to_vec(),
            count_limit: None,
            candidates: candidates
                .iter()
                .map(|(count, claims)| CapabilityCandidate {
                    count: *count,
                    claims: *claims,
                })
                .collect(),
        }
    }

    #[test]
    fn failed_candidate_returns_every_partial_claim_before_next_priority() {
        let limit = claims(7, 100);
        let profile = ProductCapabilityProfile {
            shared_runtime_candidates: vec![limit],
            closures: vec![
                closure(DemuxBase, &[], &[(1, claims(1, 0))]),
                closure(
                    TsFilter,
                    &[DemuxBase],
                    &[(5, claims(5, 101)), (3, claims(3, 30))],
                ),
                closure(PlaybackDvr, &[DemuxBase], &[(3, claims(3, 70))]),
            ],
        };
        let selected = select_capabilities(&profile, limit, |_| Ok(())).unwrap();
        assert_eq!(selected.count(TsFilter), 3);
        assert_eq!(selected.count(PlaybackDvr), 3);
        assert_eq!(selected.claims(), limit);
    }

    #[test]
    fn suppressed_local_closure_does_not_suppress_unrelated_dvr() {
        let limit = claims(4, 10);
        let profile = ProductCapabilityProfile {
            shared_runtime_candidates: vec![limit],
            closures: vec![
                closure(DemuxBase, &[], &[(1, claims(1, 0))]),
                closure(TsFilter, &[DemuxBase], &[(1, claims(1, 0))]),
                closure(AudioAv, &[TsFilter], &[(1, claims(4, 1))]),
                closure(PlaybackDvr, &[DemuxBase], &[(2, claims(2, 10))]),
            ],
        };
        let selected = select_capabilities(&profile, limit, |_| Ok(())).unwrap();
        assert_eq!(selected.count(AudioAv), 0);
        assert_eq!(selected.count(PlaybackDvr), 2);
    }

    #[test]
    fn global_validation_failure_returns_claims_in_reverse_order() {
        let limit = claims(10, 100);
        let profile = ProductCapabilityProfile {
            shared_runtime_candidates: vec![limit],
            closures: vec![
                closure(DemuxBase, &[], &[(1, claims(1, 0))]),
                closure(TsFilter, &[DemuxBase], &[(3, claims(3, 30))]),
                closure(PlaybackDvr, &[DemuxBase], &[(2, claims(2, 20))]),
            ],
        };
        let error = select_capabilities(&profile, limit, |_| Err("横断不整合")).unwrap_err();
        assert_eq!(
            error.returned_in_order,
            vec![PlaybackDvr, TsFilter, DemuxBase]
        );
        assert_eq!(
            select_capabilities(&profile, limit, |_| Ok(()))
                .unwrap()
                .claims(),
            claims(6, 50)
        );
    }

    #[test]
    fn section_tracker_and_shared_callback_claims_bound_the_selected_count() {
        let limit = CapabilityClaims::default()
            .with(Worker, 9)
            .with(Callback, 3)
            .with(Reaper, 8)
            .with(SectionTracker, 3);
        let per_filter = CapabilityClaims::default()
            .with(Worker, 1)
            .with(Callback, 1)
            .with(Reaper, 2)
            .with(SectionTracker, 1);
        let candidates: Vec<_> = (1..=7)
            .rev()
            .map(|count| (count, per_filter.checked_scale(count).unwrap()))
            .collect();
        let profile = ProductCapabilityProfile {
            shared_runtime_candidates: vec![limit],
            closures: vec![closure(SectionFilter, &[], &candidates)],
        };
        assert_eq!(
            select_capabilities(&profile, limit, |_| Ok(()))
                .unwrap()
                .count(SectionFilter),
            3
        );
    }

    #[test]
    fn missing_dependency_and_arithmetic_overflow_never_reserve() {
        assert!(claims(usize::MAX, 0).checked_scale(2).is_none());
        let limit = claims(1, 1);
        let profile = ProductCapabilityProfile {
            shared_runtime_candidates: vec![limit],
            closures: vec![closure(TsFilter, &[DemuxBase], &[(1, limit)])],
        };
        let error = select_capabilities(&profile, limit, |_| Ok(())).unwrap_err();
        assert!(error.returned_in_order.is_empty());
    }
}
