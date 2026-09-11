use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct AvSyncRegistry {
    pcr_filter_ids: BTreeSet<i32>,
    media_filter_ids: BTreeSet<i32>,
    hw_sync_id_by_media_filter_id: BTreeMap<i32, i32>,
    media_filter_ids_by_hw_sync_id: BTreeMap<i32, BTreeSet<i32>>,
}

#[derive(Debug)]
#[must_use = "この準備済み一回限り権限は型付き完了入口で消費する必要があります"]
pub(crate) struct PreparedAvSyncRegistryMutation {
    candidate: AvSyncRegistry,
}

impl AvSyncRegistry {
    pub(crate) fn prepare_register_pcr_filter(
        &self,
        filter_id: i32,
    ) -> Result<PreparedAvSyncRegistryMutation, &'static str> {
        let mut candidate = self.clone();
        candidate.register_pcr_filter(filter_id)?;
        Ok(PreparedAvSyncRegistryMutation { candidate })
    }

    pub(crate) fn prepare_register_media_filter(
        &self,
        filter_id: i32,
    ) -> Result<PreparedAvSyncRegistryMutation, &'static str> {
        let mut candidate = self.clone();
        candidate.register_media_filter(filter_id)?;
        Ok(PreparedAvSyncRegistryMutation { candidate })
    }

    pub(crate) fn prepare_unregister_filter(
        &self,
        filter_id: i32,
    ) -> PreparedAvSyncRegistryMutation {
        let mut candidate = self.clone();
        candidate.unregister_filter(filter_id);
        PreparedAvSyncRegistryMutation { candidate }
    }

    pub(crate) fn commit(&mut self, prepared: PreparedAvSyncRegistryMutation) {
        *self = prepared.candidate;
    }

    pub(crate) fn register_pcr_filter(&mut self, filter_id: i32) -> Result<(), &'static str> {
        if filter_id < 0 {
            return Err("PCR filter id must be non-negative");
        }
        self.pcr_filter_ids.insert(filter_id);
        let unbound_media = self
            .media_filter_ids
            .iter()
            .copied()
            .filter(|media_filter_id| {
                !self
                    .hw_sync_id_by_media_filter_id
                    .contains_key(media_filter_id)
            })
            .collect::<Vec<_>>();
        for media_filter_id in unbound_media {
            self.bind_media_filter(media_filter_id, filter_id);
        }
        Ok(())
    }

    pub(crate) fn register_media_filter(
        &mut self,
        filter_id: i32,
    ) -> Result<Option<i32>, &'static str> {
        if filter_id < 0 {
            return Err("media filter id must be non-negative");
        }
        self.remove_media_relation(filter_id);
        self.media_filter_ids.insert(filter_id);
        let hw_sync_id = self.pcr_filter_ids.first().copied();
        if let Some(hw_sync_id) = hw_sync_id {
            self.bind_media_filter(filter_id, hw_sync_id);
        }
        Ok(hw_sync_id)
    }

    pub(crate) fn unregister_filter(&mut self, filter_id: i32) {
        self.media_filter_ids.remove(&filter_id);
        self.remove_media_relation(filter_id);
        if !self.pcr_filter_ids.remove(&filter_id) {
            return;
        }
        let affected_media = self
            .media_filter_ids_by_hw_sync_id
            .remove(&filter_id)
            .unwrap_or_default();
        for media_filter_id in &affected_media {
            self.hw_sync_id_by_media_filter_id.remove(media_filter_id);
        }
        if let Some(replacement_hw_sync_id) = self.pcr_filter_ids.first().copied() {
            for media_filter_id in affected_media {
                self.bind_media_filter(media_filter_id, replacement_hw_sync_id);
            }
        }
    }

    pub(crate) fn hw_sync_id_for_media_filter(&self, filter_id: i32) -> Option<i32> {
        let hw_sync_id = *self.hw_sync_id_by_media_filter_id.get(&filter_id)?;
        self.pcr_filter_ids
            .contains(&hw_sync_id)
            .then_some(hw_sync_id)
    }

    pub(crate) fn pcr_filter_id_for_hw_sync_id(&self, hw_sync_id: i32) -> Option<i32> {
        self.pcr_filter_ids
            .contains(&hw_sync_id)
            .then_some(hw_sync_id)
    }

    fn bind_media_filter(&mut self, media_filter_id: i32, hw_sync_id: i32) {
        self.hw_sync_id_by_media_filter_id
            .insert(media_filter_id, hw_sync_id);
        self.media_filter_ids_by_hw_sync_id
            .entry(hw_sync_id)
            .or_default()
            .insert(media_filter_id);
    }

    fn remove_media_relation(&mut self, media_filter_id: i32) {
        let Some(hw_sync_id) = self.hw_sync_id_by_media_filter_id.remove(&media_filter_id) else {
            return;
        };
        let remove_reverse_entry = if let Some(media_filter_ids) =
            self.media_filter_ids_by_hw_sync_id.get_mut(&hw_sync_id)
        {
            media_filter_ids.remove(&media_filter_id);
            media_filter_ids.is_empty()
        } else {
            false
        };
        if remove_reverse_entry {
            self.media_filter_ids_by_hw_sync_id.remove(&hw_sync_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepared_mutation_aborts_without_changing_relations() {
        let registry = AvSyncRegistry::default();
        let prepared = registry.prepare_register_pcr_filter(4).unwrap();
        drop(prepared);
        assert_eq!(registry.pcr_filter_id_for_hw_sync_id(4), None);
    }

    #[test]
    fn many_media_filters_share_one_hw_sync_relation() {
        let mut registry = AvSyncRegistry::default();
        let pcr = registry.prepare_register_pcr_filter(4).unwrap();
        registry.commit(pcr);
        let first = registry.prepare_register_media_filter(10).unwrap();
        registry.commit(first);
        let second = registry.prepare_register_media_filter(11).unwrap();
        registry.commit(second);
        let remove = registry.prepare_unregister_filter(10);
        registry.commit(remove);
        assert_eq!(registry.hw_sync_id_for_media_filter(10), None);
        assert_eq!(registry.hw_sync_id_for_media_filter(11), Some(4));
    }
}
