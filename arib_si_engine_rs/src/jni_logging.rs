use android_logger::{Config, FilterBuilder};

fn configuration() -> Config {
    // タグ短縮前のRust targetで選別し、他モジュールの詳細ログは維持する。
    Config::default()
        .with_max_level(log::LevelFilter::Trace)
        .with_filter(FilterBuilder::new().parse("trace,jni::=debug").build())
}

#[no_mangle]
pub extern "system" fn JNI_OnLoad(
    _vm: *mut jni::sys::JavaVM,
    _reserved: *mut std::ffi::c_void,
) -> jni::sys::jint {
    android_logger::init_once(configuration());
    jni::sys::JNI_VERSION_1_6
}

#[cfg(test)]
mod tests {
    use super::configuration;
    use android_logger::AndroidLogger;
    use log::{Level, Log, Record};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn jni_trace_is_filtered_before_formatting_without_hiding_other_diagnostics() {
        let formatted = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&formatted);
        let logger = AndroidLogger::new(configuration().format(move |_, _| {
            observed.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }));
        for target in ["jni::wrapper::jnienv", "jni::wrapper::objects::jvalue"] {
            for level in [
                Level::Trace,
                Level::Debug,
                Level::Info,
                Level::Warn,
                Level::Error,
            ] {
                let before = formatted.load(Ordering::Relaxed);
                logger.log(
                    &Record::builder()
                        .target(target)
                        .module_path(Some(target))
                        .level(level)
                        .args(format_args!("probe"))
                        .build(),
                );
                assert_eq!(
                    formatted.load(Ordering::Relaxed) - before,
                    usize::from(level != Level::Trace)
                );
            }
        }
        for target in ["maleicacid_arib_si_engine_jni", "jni_other"] {
            let before = formatted.load(Ordering::Relaxed);
            logger.log(
                &Record::builder()
                    .target(target)
                    .module_path(Some(target))
                    .level(Level::Trace)
                    .args(format_args!("probe"))
                    .build(),
            );
            assert_eq!(formatted.load(Ordering::Relaxed), before + 1);
        }
    }
}
