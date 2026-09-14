// SPDX-License-Identifier: GPL-3.0-only
#include "KeyRegistry.h"

#include <algorithm>
#include <sys/random.h>
#include <time.h>

namespace maleicacid::cas {

uint32_t todayMjd() {
    const auto now = time(nullptr);
    if (now < 0) return UINT32_MAX;
    const uint64_t days = static_cast<uint64_t>(now) / 86400 + 40587;
    return days > UINT32_MAX ? UINT32_MAX : static_cast<uint32_t>(days);
}

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
        auto slot = std::make_shared<Slot>(token);
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
    slot->ready = false;
    eraseSecret(slot->keys.bytes.data(), slot->keys.bytes.size());
}

Result KeyRegistry::update(const std::shared_ptr<Slot>& slot, const Secret<16>& keys,
                           uint8_t group, uint32_t expires) {
    std::lock_guard lock(mutex_);
    if (!slot->live) return Result::SessionClosed;
    slot->keys = keys;
    slot->group = group;
    slot->expires = expires;
    slot->ready = true;
    return Result::Ok;
}

Result KeyRegistry::resolve(const Token& token, Secret<16>* keys) {
    if (keys == nullptr) return Result::BadValue;
    eraseSecret(keys->bytes.data(), keys->bytes.size());
    std::lock_guard lock(mutex_);
    for (const auto& entry : slots_) {
        const auto slot = entry.lock();
        if (!slot || !slot->live || slot->token != token) continue;
        if (!slot->ready) return Result::NoLicense;
        if (todayMjd() > slot->expires) {
            slot->ready = false;
            eraseSecret(slot->keys.bytes.data(), slot->keys.bytes.size());
            return Result::Expired;
        }
        *keys = slot->keys;
        return Result::Ok;
    }
    return Result::SessionClosed;
}

void KeyRegistry::invalidateGroup(uint8_t group) {
    std::lock_guard lock(mutex_);
    for (const auto& entry : slots_) {
        auto slot = entry.lock();
        if (slot && slot->group == group) {
            slot->ready = false;
            eraseSecret(slot->keys.bytes.data(), slot->keys.bytes.size());
        }
    }
}

void KeyRegistry::revokeAll() {
    std::lock_guard lock(mutex_);
    for (const auto& entry : slots_) {
        auto slot = entry.lock();
        if (slot) {
            slot->live = false;
            slot->ready = false;
            eraseSecret(slot->keys.bytes.data(), slot->keys.bytes.size());
        }
    }
}

}  // namespace maleicacid::cas
