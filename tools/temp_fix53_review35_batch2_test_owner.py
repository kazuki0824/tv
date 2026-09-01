from pathlib import Path
import re

# Replace tests that intentionally reconstructed the old device-local owner
# internals. The replacement still verifies that abnormal worker termination
# cannot be converted to success, but only through the canonical start path.
p=Path('tuner_hal2/device/src/runtime/live_pump.rs'); t=p.read_text()
pat=r'''        let result: Arc<Mutex<Option<Result<FrontendLivePumpReport, HalError>>>> =\n            Arc::new\(Mutex::new\(None\)\);\n        let join = std::thread::spawn\(\|\| \{\}\);\n        let owner = FrontendLivePumpOwner \{\n            cancel: Arc::new\(AtomicBool::new\(false\)\),\n            thread_result: ThreadResultOwner::new_for_test\(\n                "[^"]+",\n                result,\n                Some\(join\),\n            \),\n        \};'''
rep='''        let owner = FrontendLivePumpOwner {\n            cancel: Arc::new(AtomicBool::new(false)),\n            thread_result: ThreadResultOwner::start(\n                "live-pump-owner-failure-test",\n                || -> Result<FrontendLivePumpReport, HalError> {\n                    panic!("forced worker owner failure")\n                },\n            )\n            .unwrap(),\n        };'''
t,n=re.subn(pat,rep,t)
if n!=2: raise SystemExit(f'live pump old-owner test blocks: expected 2, got {n}')
p.write_text(t)

p=Path('tuner_hal2/device/src/runtime/frontend_worker.rs'); t=p.read_text()
pat=r'''        type WorkerThreadResult =\n            Arc<Mutex<Option<Result<\(Result<\(\), HalError>, WorkerExit\), HalError>>>>;\n        let result: WorkerThreadResult = Arc::new\(Mutex::new\(None\)\);\n        let join = std::thread::spawn\(\|\| \{\}\);\n        registry.slots.insert\(\n            key,\n            FrontendWorkerSlot \{\n                generation: 10,\n                cancel: Arc::new\(AtomicBool::new\(false\)\),\n                cancel_reason: Arc::new\(Mutex::new\(None\)\),\n                thread_result: Some\(ThreadResultOwner::new_for_test\(\n                    "frontend-worker-missing-test",\n                    result,\n                    Some\(join\),\n                \)\),\n                pending_completed: None,\n            \},\n        \);'''
rep='''        registry.slots.insert(\n            key,\n            FrontendWorkerSlot {\n                generation: 10,\n                cancel: Arc::new(AtomicBool::new(false)),\n                cancel_reason: Arc::new(Mutex::new(None)),\n                thread_result: Some(\n                    ThreadResultOwner::start(\n                        "frontend-worker-owner-failure-test",\n                        || -> Result<(Result<(), HalError>, WorkerExit), HalError> {\n                            panic!("forced worker owner failure")\n                        },\n                    )\n                    .unwrap(),\n                ),\n                pending_completed: None,\n            },\n        );'''
t,n=re.subn(pat,rep,t)
if n!=1: raise SystemExit(f'frontend worker old-owner test block: expected 1, got {n}')
p.write_text(t)

# WorkerRuntime implements Drop, so the shared physical owner is optional and
# explicitly taken during join instead of moving a field out of a Drop type.
p=Path('tuner_hal2/service_runtime/src/worker_runtime.rs'); t=p.read_text()
t=t.replace('    handle: WorkerRuntimeResultOwner<WorkerTerminalResult<T>, ()>,\n','    handle: Option<WorkerRuntimeResultOwner<WorkerTerminalResult<T>, ()>>,\n')
t=t.replace('self.finished.load(Ordering::Acquire) || self.handle.is_thread_finished()','self.finished.load(Ordering::Acquire)\n            || self\n                .handle\n                .as_ref()\n                .map(|handle| handle.is_thread_finished())\n                .unwrap_or(true)')
t=t.replace('            self.handle.unpark();','            if let Some(handle) = self.handle.as_ref() {\n                handle.unpark();\n            }')
old='''    pub(crate) fn join(self) -> WorkerTerminalResult<T> {\n        match self.handle.join_after_stop() {\n            Ok(Ok(result)) => result,\n            Ok(Err(())) | Err(_) => WorkerTerminalResult::PanicOrJoinFailure,\n        }\n    }\n'''
new='''    pub(crate) fn join(mut self) -> WorkerTerminalResult<T> {\n        let Some(handle) = self.handle.take() else {\n            return WorkerTerminalResult::PanicOrJoinFailure;\n        };\n        match handle.join_after_stop() {\n            Ok(Ok(result)) => result,\n            Ok(Err(())) | Err(_) => WorkerTerminalResult::PanicOrJoinFailure,\n        }\n    }\n'''
if t.count(old)!=1: raise SystemExit(f'join anchor count {t.count(old)}')
t=t.replace(old,new,1)
if t.count('            handle,\n')<1: raise SystemExit('spawn handle initializer missing')
t=t.replace('            handle,\n','            handle: Some(handle),\n',1)
p.write_text(t)
print('worker owner tests and Drop-safe ownership aligned')