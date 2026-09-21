// SPDX-License-Identifier: GPL-3.0-only
#pragma once

#include "Types.h"

#include <memory>
#include <mutex>
#include <set>

namespace maleicacid::cas {

class KeyRegistry {
public:
    // Slot の所持が更新権限であり、外部から受け取る token は読取り専用である。
    struct Slot {
        const Token token;
        explicit Slot(Token identity) : token(identity) {}
    private:
        friend class KeyRegistry;
        Secret<16> keys;
        bool live = true;
        bool ready = false;
        uint8_t group = 0;
    };

    static KeyRegistry& instance();
    Result open(std::shared_ptr<Slot>* slot);
    void close(const std::shared_ptr<Slot>& slot);
    Result update(const std::shared_ptr<Slot>& slot, const Secret<16>& keys, uint8_t group);
    Result resolve(const Token& token, Secret<16>* keys);
    void invalidateGroup(uint8_t group);
    void revokeAll();

private:
    std::mutex mutex_;
    std::array<std::weak_ptr<Slot>, kSessionCapacity> slots_;
    std::set<Token> issued_;
};

}  // namespace maleicacid::cas
