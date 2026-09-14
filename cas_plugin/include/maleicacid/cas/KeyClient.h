// SPDX-License-Identifier: GPL-3.0-only
#pragma once

#include <array>
#include <cstddef>
#include <cstdint>

namespace maleicacid::cas {

// Tuner 内部 consumer 用。packet ごとに取得し、別 packet へキャッシュしない。
struct PacketKeys {
    std::array<uint8_t, 8> odd{};
    std::array<uint8_t, 8> even{};
    ~PacketKeys();
};

enum class KeyResult { Ok, InvalidToken, UnknownToken, Unavailable };
KeyResult acquirePacketKeys(const uint8_t* token, size_t length, PacketKeys* output);

}  // namespace maleicacid::cas
