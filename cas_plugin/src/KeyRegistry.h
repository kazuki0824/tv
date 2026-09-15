// SPDX-License-Identifier: GPL-3.0-only
#pragma once

#include "Types.h"
#include "SharedSlot.h"

#include <memory>
#include <mutex>
#include <set>

namespace maleicacid::cas {

class KeyRegistry {
public:
    // Slot の所持が更新権限であり、外部から受け取る token は読取り専用である。
    struct Slot {
        const Token token;
        Slot(Token identity, std::unique_ptr<SharedSlot> shared)
            : token(identity), shared(std::move(shared)) {}
    private:
        friend class KeyRegistry;
        std::unique_ptr<SharedSlot> shared;
        bool live = true;
        uint8_t group = 0;
    };

    static KeyRegistry& instance();
    Result open(std::shared_ptr<Slot>* slot);
    void close(const std::shared_ptr<Slot>& slot);
    Result update(const std::shared_ptr<Slot>& slot, const Secret<16>& keys,
                  uint8_t group, uint32_t expires);
#ifdef MALEICACID_CAS_TEST
    Result resolve(const Token& token, Secret<16>* keys);
#endif
    Result bind(const Token& token, int* readerFd);
    void invalidateGroup(uint8_t group);
    void revokeAll();

private:
    std::mutex mutex_;
    std::array<std::weak_ptr<Slot>, kSessionCapacity> slots_;
    std::set<Token> issued_;
};

}  // namespace maleicacid::cas
