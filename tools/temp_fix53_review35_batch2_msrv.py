from pathlib import Path
p=Path('tuner_hal2/control/src/lib.rs')
t=p.read_text()
old='self.join.as_ref().is_none_or(|handle| handle.is_finished())'
new='self.join.as_ref().map(|handle| handle.is_finished()).unwrap_or(true)'
if t.count(old)!=1: raise SystemExit(f'expected one is_none_or, got {t.count(old)}')
p.write_text(t.replace(old,new,1))
print('Rust 1.81 compatibility applied')