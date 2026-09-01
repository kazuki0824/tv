from pathlib import Path
R=Path('tuner_hal2')
def one(p,o,n):
 t=p.read_text(); c=t.count(o)
 if c!=1: raise SystemExit(f'{p}: anchor count {c}')
 p.write_text(t.replace(o,n,1))

p=R/'aidl_service/src/dvr_callback_delivery.rs'
one(p,'''        guard.finish_callback_delivery_failure_use_case(CallbackDeliveryFailureReport::dvr(
            handle.object_id(),
            handle.generation(),
            phase,
            dvr_phase,
            primary.clone(),
        ))
''','''        if phase == CallbackDeliveryFailurePhase::PostCommitNotification {
            guard.finish_dvr_post_commit_notification_failure_use_case(
                handle.object_id(),
                handle.generation(),
                dvr_phase,
                primary.clone(),
            )
        } else {
            guard.finish_callback_delivery_failure_use_case(CallbackDeliveryFailureReport::dvr(
                handle.object_id(),
                handle.generation(),
                phase,
                dvr_phase,
                primary.clone(),
            ))
        }
''')
p=R/'service_runtime/src/boot.rs'
t=p.read_text(); anchor='    pub fn finish_callback_delivery_failure_use_case(\n'
if t.count(anchor)!=1: raise SystemExit('callback failure anchor')
method='''    pub fn finish_dvr_post_commit_notification_failure_use_case(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        phase: DvrPostCommitNotificationPhase,
        primary: HalError,
    ) -> Result<(), HalError> {
        let service_critical = phase == DvrPostCommitNotificationPhase::StatusNotifierStart
            && self
                .dvr_status_metadata_snapshot_for_aidl_object(object_id, generation)
                .map(|snapshot| snapshot.is_playback)
                .unwrap_or(true);
        self.finish_callback_delivery_failure_use_case(CallbackDeliveryFailureReport::dvr(
            object_id,
            generation,
            CallbackDeliveryFailurePhase::PostCommitNotification,
            phase,
            primary,
        ))?;
        if service_critical {
            self.mark_service_critical();
        }
        Ok(())
    }

'''
p.write_text(t.replace(anchor,method+anchor,1))
print('S-05 patch applied')
