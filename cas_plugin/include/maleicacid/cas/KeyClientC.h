// SPDX-License-Identifier: GPL-3.0-only
#pragma once

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

enum MaleicacidCasKeyResult {
    MALEICACID_CAS_KEY_OK = 0,
    MALEICACID_CAS_KEY_INVALID_TOKEN = 1,
    MALEICACID_CAS_KEY_UNKNOWN_TOKEN = 2,
    MALEICACID_CAS_KEY_UNAVAILABLE = 3,
};

enum MaleicacidCasScheme {
    MALEICACID_CAS_SCHEME_UNKNOWN = 0,
    MALEICACID_CAS_SCHEME_B25 = 1,
    MALEICACID_CAS_SCHEME_B1 = 2,
};

// 成功時の参照は呼出し元が所有し、release まで複数 thread から読取り可能。
// token は bind 呼出し中だけ参照する。失敗時は *reference を NULL にする。
int maleicacid_cas_bind_key_reference(const uint8_t* token, size_t length, void** reference);
// 参照に結び付いたCAS方式を返す。無効な参照ではUNKNOWNを返す。
uint8_t maleicacid_cas_key_reference_scheme(const void* reference);
void maleicacid_cas_release_key_reference(void* reference);
// odd/even は各8 byteの書込み可能領域。成功時以外は両方をゼロ化する。
// reference の release とこの呼出しを競合させてはならない。
int maleicacid_cas_snapshot_key_reference(const void* reference, uint8_t* odd, uint8_t* even);

#ifdef __cplusplus
}
#endif
