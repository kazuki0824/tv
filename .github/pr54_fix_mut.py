from pathlib import Path
p=Path('arib_si_engine_rs/src/lib.rs')
t=p.read_text()
old='''    match parser.lock() {
        Ok(mut guard) => f(&mut guard),
        Err(_) => {
            record_si_mutex_poison(SI_PARSER_LOCK_NAME);
            STATUS_INTERNAL_ERROR
        }
    }
}'''
new='''    let result = match parser.lock() {
        Ok(mut guard) => f(&mut guard),
        Err(_) => {
            record_si_mutex_poison(SI_PARSER_LOCK_NAME);
            STATUS_INTERNAL_ERROR
        }
    };
    result
}'''
if t.count(old)!=1:
    raise SystemExit(f'match count={t.count(old)}')
p.write_text(t.replace(old,new,1))
