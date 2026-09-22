#![cfg(test)]
// 製品側の呼出元を含まない独立試験なので、未使用警告の抑止はこの試験対象だけに限定する。
#![allow(dead_code)]

// 既存の正本型をそのまま検査するため、この試験対象では表記だけの指摘を対象外にする。
#[allow(clippy::derivable_impls)]
mod descrambler_key_table;
#[allow(clippy::type_complexity)]
mod descrambler_session;
#[allow(clippy::enum_variant_names)]
mod diagnostics;
#[path = "boot/demux_error.rs"]
mod demux_error;
mod playback_consume_txn;
mod registry;

// ホストには DMA ヒープがない。確保要求は成功と偽らず未対応として返す。
#[cfg(not(target_os = "android"))]
#[no_mangle]
extern "C" fn tuner_dmabuf_heap_alloc_system(_len: usize) -> i32 {
    -38
}
