// SPDX-License-Identifier: GPL-3.0-only
#pragma once

#include <array>
#include <cstddef>
#include <cstdint>
#include <memory>

namespace maleicacid::cas {

// 取得済みの1 packet だけで使用し、別 packet へ持ち越さない。
struct PacketKeys {
    std::array<uint8_t, 8> odd{};
    std::array<uint8_t, 8> even{};
    ~PacketKeys();
};

enum class KeyResult { Ok, InvalidToken, UnknownToken, Unavailable };

struct SharedKeyState;
class KeyReference {
public:
    ~KeyReference();
    KeyReference(const KeyReference&) = delete;
    KeyReference& operator=(const KeyReference&) = delete;
    // CAS への通信は行わず、結合済み共有領域と所有者の生存状態を確認する。
    KeyResult snapshot(PacketKeys* output) const;
    static KeyResult bind(const uint8_t* token, size_t length,
                          std::unique_ptr<KeyReference>* output);
#ifdef MALEICACID_CAS_TEST
    static uint64_t bindingQueriesForTest();
#endif
private:
    KeyReference(const SharedKeyState* state, int owner) : state_(state), owner_(owner) {}
    const SharedKeyState* state_;
    int owner_;
};

}  // namespace maleicacid::cas
