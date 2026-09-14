// SPDX-License-Identifier: GPL-3.0-only
#pragma once

#include <array>
#include <cstddef>
#include <cstdint>
#include <vector>

namespace maleicacid::cas {

enum class Result {
    Ok, BadValue, Unsupported, NotProvisioned, NoLicense, Expired,
    SessionClosed, Busy, Decrypt, Revoked, InvalidState, Unknown,
};

using Bytes = std::vector<uint8_t>;
using Token = std::array<uint8_t, 16>;
constexpr int32_t kB25SystemId = 0x0005;
constexpr int32_t kSessionCapacity = 64;

inline void eraseSecret(void* data, size_t size) {
    auto* p = static_cast<volatile uint8_t*>(data);
    while (size-- != 0) *p++ = 0;
}

template <size_t N> struct Secret {
    std::array<uint8_t, N> bytes{};
    ~Secret() { eraseSecret(bytes.data(), bytes.size()); }
};

struct View {
    const uint8_t* data = nullptr;
    size_t size = 0;
    uint8_t operator[](size_t i) const { return data[i]; }
};

inline uint16_t be16(const uint8_t* p) {
    return static_cast<uint16_t>((static_cast<uint16_t>(p[0]) << 8) | p[1]);
}

// MJD は信頼する受信機時計から取得する。未知の時計を期限内と見なさない。
uint32_t todayMjd();

}  // namespace maleicacid::cas
