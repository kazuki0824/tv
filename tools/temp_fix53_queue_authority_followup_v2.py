from pathlib import Path

script_path = Path("tools/temp_fix53_queue_authority_followup.py")
source = script_path.read_text()
old = '''# Validate the reviewed structural contracts after all edits.
queue_text = QUEUE.read_text()
for forbidden in [
    "if self.active && self.release().is_err()",
    "if self.active && self.rollback().is_err()",
]:
    if forbidden in queue_text:
        raise SystemExit(f"queue Drop still contains fallible cleanup: {forbidden}")
if "fn fail_close_unconsumed_authority(&self)" not in queue_text:
    raise SystemExit("infallible queue authority fail-close helper missing")
'''
new = r'''# Validate only the two reviewed queue-authority Drop implementations.
queue_text = QUEUE.read_text()
for type_name, forbidden in [
    ("QueueEpochToken", "self.release()"),
    ("QueueEpochDrainTxn", "self.rollback()"),
]:
    start = queue_text.index(f"impl Drop for {type_name} {{")
    next_item = queue_text.find("\n}\n", start)
    if next_item < 0:
        raise SystemExit(f"{type_name}: Drop block terminator not found")
    block = queue_text[start:next_item + 3]
    if forbidden in block:
        raise SystemExit(f"{type_name}: Drop still calls fallible cleanup: {forbidden}")
    if "fail_close_unconsumed_authority()" not in block:
        raise SystemExit(f"{type_name}: Drop no longer uses fail-closed backstop")
if "fn fail_close_unconsumed_authority(&self)" not in queue_text:
    raise SystemExit("infallible queue authority fail-close helper missing")
'''
if source.count(old) != 1:
    raise SystemExit("reviewed validation block not found exactly once")
exec(compile(source.replace(old, new, 1), str(script_path), "exec"))
