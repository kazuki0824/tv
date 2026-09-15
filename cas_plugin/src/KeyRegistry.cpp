// SPDX-License-Identifier: GPL-3.0-only
#include "KeyRegistry.h"

#include <algorithm>
#include <sys/random.h>

namespace maleicacid::cas {

KeyRegistry& KeyRegistry::instance() {
    static KeyRegistry registry;
    return registry;
}

Result KeyRegistry::open(std::shared_ptr<Slot>* result) {
    if (result == nullptr) return Result::BadValue;
    result->reset();
    std::lock_guard lock(mutex_);
    auto empty = slots_.end();
    for (auto it = slots_.begin(); it != slots_.end(); ++it) {
        auto slot = it->lock();
        if (!slot || !slot->live) { empty = it; break; }
    }
    if (empty == slots_.end()) return Result::Busy;
    Token token;
    // 乱数源が未準備なら待ち続けず失敗させる。発行済み ID は再利用しない。
    for (int attempt = 0; attempt < 8; ++attempt) {
        if (getrandom(token.data(), token.size(), GRND_NONBLOCK) !=
            static_cast<ssize_t>(token.size())) return Result::Busy;
        if (issued_.count(token) || std::all_of(token.begin(), token.end(),
                                               [](uint8_t b) { return b == 0; })) continue;
        auto shared = SharedSlot::create();
        if (!shared) return Result::Busy;
        auto slot = std::make_shared<Slot>(token, std::move(shared));
        issued_.insert(token);
        *empty = slot;
        *result = std::move(slot);
        return Result::Ok;
    }
    return Result::Busy;
}

void KeyRegistry::close(const std::shared_ptr<Slot>& slot) {
    std::lock_guard lock(mutex_);
    slot->live = false;
    slot->shared->state().store(nullptr, 0);
}

Result KeyRegistry::update(const std::shared_ptr<Slot>& slot, const Secret<16>& keys,
                           uint8_t group, uint32_t expires) {
    std::lock_guard lock(mutex_);
    if (!slot->live) return Result::SessionClosed;
    slot->group = group;
    if (!slot->shared->state().store(&keys, expires)) {
        slot->live = false;
        return Result::Revoked;
    }
    return Result::Ok;
}

#ifdef MALEICACID_CAS_TEST
Result KeyRegistry::resolve(const Token& token, Secret<16>* keys) {
    if (keys == nullptr) return Result::BadValue;
    eraseSecret(keys->bytes.data(), keys->bytes.size());
    std::lock_guard lock(mutex_);
    for (const auto& entry : slots_) {
        const auto slot = entry.lock();
        if (!slot || !slot->live || slot->token != token) continue;
        const auto result = slot->shared->state().snapshot(keys);
        if (result == Result::Expired) slot->shared->state().store(nullptr, 0);
        return result;
    }
    return Result::SessionClosed;
}
#endif

Result KeyRegistry::bind(const Token& token, int* readerFd) {
    if (readerFd == nullptr) return Result::BadValue;
    *readerFd = -1;
    std::lock_guard lock(mutex_);
    for (const auto& entry : slots_) {
        const auto slot = entry.lock();
        if (!slot || !slot->live || slot->token != token) continue;
        Secret<16> current;
        const auto result = slot->shared->state().snapshot(&current);
        if (result != Result::Ok) return result;
        *readerFd = slot->shared->readerFd();
        return *readerFd >= 0 ? Result::Ok : Result::Busy;
    }
    return Result::SessionClosed;
}

void KeyRegistry::invalidateGroup(uint8_t group) {
    std::lock_guard lock(mutex_);
    for (const auto& entry : slots_) {
        auto slot = entry.lock();
        if (slot && slot->group == group) {
            slot->shared->state().store(nullptr, 0);
        }
    }
}

void KeyRegistry::revokeAll() {
    std::lock_guard lock(mutex_);
    for (const auto& entry : slots_) {
        auto slot = entry.lock();
        if (slot) {
            slot->live = false;
            slot->shared->state().store(nullptr, 0);
        }
    }
}

}  // namespace maleicacid::cas
