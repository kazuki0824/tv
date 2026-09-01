from pathlib import Path

# Replace white-box construction of the old device-local result owner with the
# canonical public start path. These tests still assert that abnormal worker
# termination never becomes success, without reconstructing hidden owner state.
for rel in [
    'tuner_hal2/device/src/runtime/live_pump.rs',
    'tuner_hal2/device/src/runtime/frontend_worker.rs',
]:
    p=Path(rel); t=p.read_text()
    while 'ThreadResultOwner::new_for_test(' in t:
        start=t.index('ThreadResultOwner::new_for_test(')
        end=t.index(')', t.index('Some(join)', start)) + 1
        replacement='ThreadResultOwner::start("worker-owner-failure-test", || -> Result<_, HalError> { panic!("forced worker owner failure") }).unwrap()'
        t=t[:start]+replacement+t[end:]
    p.write_text(t)

# WorkerRuntime implements Drop, so its canonical physical owner is optional and
# explicitly taken during join; this avoids moving a field out of a Drop type.
p=Path('tuner_hal2/service_runtime/src/worker_runtime.rs'); t=p.read_text()
t=t.replace('    handle: WorkerRuntimeResultOwner<WorkerTerminalResult<T>, ()>,\n','    handle: Option<WorkerRuntimeResultOwner<WorkerTerminalResult<T>, ()>>,\n')
t=t.replace('self.finished.load(Ordering::Acquire) || self.handle.is_thread_finished()','self.finished.load(Ordering::Acquire)\n            || self.handle.as_ref().map(|handle| handle.is_thread_finished()).unwrap_or(true)')
t=t.replace('            self.handle.unpark();','            if let Some(handle) = self.handle.as_ref() {\n                handle.unpark();\n            }')
old='''    pub(crate) fn join(self) -> WorkerTerminalResult<T> {
        match self.handle.join_after_stop() {
            Ok(Ok(result)) => result,
            Ok(Err(())) | Err(_) => WorkerTerminalResult::PanicOrJoinFailure,
        }
    }
'''
new='''    pub(crate) fn join(mut self) -> WorkerTerminalResult<T> {
        let Some(handle) = self.handle.take() else {
            return WorkerTerminalResult::PanicOrJoinFailure;
        };
        match handle.join_after_stop() {
            Ok(Ok(result)) => result,
            Ok(Err(())) | Err(_) => WorkerTerminalResult::PanicOrJoinFailure,
        }
    }
'''
if t.count(old)!=1: raise SystemExit(f'join anchor count {t.count(old)}')
t=t.replace(old,new,1)
t=t.replace('            handle,\n','            handle: Some(handle),\n',1)
p.write_text(t)
print('worker owner tests and Drop-safe ownership aligned')