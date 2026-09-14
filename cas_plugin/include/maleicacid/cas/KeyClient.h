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

enum class KeyResult { Ok, InvalidToken, Unavailable };
KeyResult acquirePacketKeys(const uint8_t* token, size_t length, PacketKeys* output);

// 復号側への静的リンクで参照する parameter。鍵共有の応答には含めない。
struct Multi2Parameters {
    static constexpr std::array<uint8_t, 32> systemKey{
        0x36, 0x31, 0x04, 0x66, 0x4b, 0x17, 0xea, 0x5c,
        0x32, 0xdf, 0x9c, 0xf5, 0xc4, 0xc3, 0x6c, 0x1b,
        0xec, 0x99, 0x39, 0x21, 0x68, 0x9d, 0x4b, 0xb7,
        0xb7, 0x4e, 0x40, 0x84, 0x0d, 0x2e, 0x7d, 0x98,
    };
    static constexpr std::array<uint8_t, 8> initialCbc{
        0xfe, 0x27, 0x19, 0x99, 0x19, 0x69, 0x09, 0x11,
    };
};

}  // namespace maleicacid::cas
