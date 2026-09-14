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

// pointerは呼出し中だけ参照する。odd/evenは各8 byteの書込み可能領域を要求し、
// 成功時以外は両方をゼロ化する。
int maleicacid_cas_acquire_packet_keys(const uint8_t* token, size_t length, uint8_t* odd,
                                       uint8_t* even);

#ifdef __cplusplus
}
#endif
