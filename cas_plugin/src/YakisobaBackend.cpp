// SPDX-License-Identifier: GPL-3.0-only
#include "YakisobaBackend.h"

#include <algorithm>
#include <cerrno>
#include <chrono>
#include <cstdio>
#include <cstring>
#include <fcntl.h>
#include <sys/stat.h>
#include <unistd.h>

extern "C" {
#include <Global.h>
#include <Crypto.h>
#include <Keyset.h>
#include <yakisoba.h>
int32_t Register(uint8_t group, uint8_t id, const uint8_t* key);
FILE* __real_fopen(const char* path, const char* mode);
}

namespace {
thread_local char* initialInput = nullptr;
thread_local size_t initialSize = 0;
thread_local bool initialReadFailed = false;
}

extern "C" FILE* __wrap_fopen(const char* path, const char* mode) {
    if (initialInput != nullptr) {
        auto* file = fmemopen(initialInput, initialSize, "rb");
        if (file == nullptr) initialReadFailed = true;
        return file;
    }
    return __real_fopen(path, mode);
}

namespace maleicacid::cas {
namespace {
constexpr size_t kMaxCredentialSize = 16384;
constexpr auto kLockDeadline = std::chrono::milliseconds(500);

int groupIndex(uint8_t group) {
    auto it = std::find(kYakisobaGroups.begin(), kYakisobaGroups.end(), group);
    return it == kYakisobaGroups.end() ? -1 : static_cast<int>(it - kYakisobaGroups.begin());
}

bool protocolSupported(uint8_t protocol) { return (protocol & ~0x4c) == 0; }

Result decodeResult(int result) {
    switch (result) {
        case 0: return Result::Ok;
        case -EINVAL: return Result::BadValue;
        case -EILSEQ: return Result::Decrypt;
        case -ENOKEY: return Result::NoLicense;
        default: return Result::Unknown;
    }
}

bool validInputFile(const struct stat& info) {
    return S_ISREG(info.st_mode) && info.st_size > 0 &&
           info.st_size <= static_cast<off_t>(kMaxCredentialSize);
}
}  // namespace

YakisobaBackend& YakisobaBackend::instance() {
    static YakisobaBackend owner;
    return owner;
}

#ifdef MALEICACID_CAS_TEST
void YakisobaBackend::setCredentialPathForTest(std::string path) {
    std::lock_guard lock(mutex_);
    if (!initialized_) {
        credentialPath_ = path;
        stateStore_.setPathForTest(path + ".state");
    }
}
#endif

Result YakisobaBackend::failClosed() {
    revoked_ = true;
    KeyRegistry::instance().revokeAll();
    return Result::Revoked;
}

Result YakisobaBackend::restorePersistentState(const std::array<uint8_t, 6>& cardId) {
    YakisobaPersistentState restored;
    const auto loaded = stateStore_.load(cardId, &restored);
    if (loaded == PersistentLoadResult::Missing) {
        state_.cardId = cardId;
        return Result::Ok;
    }
    if (loaded != PersistentLoadResult::Ok) return failClosed();

    for (size_t group = 0; group < kYakisobaGroups.size(); ++group) {
        for (size_t bucket = 0; bucket < kYakisobaKeyBuckets; ++bucket) {
            const auto& saved = restored.groups[group].keys[bucket];
            if (!saved.present) continue;
            Secret<8> previous;
            bool exact = false;
            for (unsigned id = bucket; id < 255; id += kYakisobaKeyBuckets) {
                if (GetKey(kYakisobaGroups[group], static_cast<uint8_t>(id), previous.bytes.data()) != 0) continue;
                if (id > saved.id) return failClosed();
                if (id == saved.id) {
                    if (previous.bytes != saved.key) return failClosed();
                    exact = true;
                }
            }
            if (!exact && Register(kYakisobaGroups[group], saved.id, saved.key.data()) != 0) {
                return failClosed();
            }
        }
    }
    state_ = restored;
    return Result::Ok;
}

Result YakisobaBackend::initialize() {
    if (revoked_) return Result::Revoked;
    if (initialized_) return Result::Ok;
    const int fd = open(credentialPath_.c_str(), O_RDONLY | O_CLOEXEC | O_NOFOLLOW | O_NONBLOCK);
    if (fd < 0) return Result::NotProvisioned;
    struct stat info {};
    if (fstat(fd, &info) || !validInputFile(info)) { close(fd); return Result::NotProvisioned; }
    Secret<kMaxCredentialSize + 1> content;
    const auto size = static_cast<size_t>(info.st_size);
    const auto count = pread(fd, content.bytes.data(), size + 1, 0);
    close(fd);
    if (count != static_cast<ssize_t>(size)) return Result::NotProvisioned;
    initialInput = reinterpret_cast<char*>(content.bytes.data());
    initialSize = size;
    initialReadFailed = false;
    const auto* card = GetCardId();
    initialInput = nullptr;
    initialSize = 0;
    if (initialReadFailed) return failClosed();
    std::array<uint8_t, 6> cardId{};
    std::copy_n(card, cardId.size(), cardId.data());
    const auto restored = restorePersistentState(cardId);
    if (restored != Result::Ok) return restored;
    initialized_ = true;
    return Result::Ok;
}

Result YakisobaBackend::validateEcm(View plain, const PersistedEntitlement& entitlement) {
    if (todayMjd() > entitlement.expires) return Result::Expired;
    if (plain[19] != 0 && plain[19] != 1) return Result::Unsupported;
    const auto broadcastDate = be16(plain.data + 20);
    if (broadcastDate != 0 && broadcastDate > entitlement.expires) return Result::Expired;
    for (size_t p = 26; p < plain.size - 4;) {
        if (plain.size - 4 - p < 2) return Result::BadValue;
        const auto tag = plain[p++];
        const auto length = plain[p++];
        if (length > plain.size - 4 - p) return Result::BadValue;
        if (tag != 0x52) return Result::Unsupported;
        if (length == 0 || length > 32) return Result::BadValue;
        bool authorized = false;
        for (size_t i = 0; i < length; ++i) authorized |= (plain[p + i] & entitlement.bitmap[i]) != 0;
        if (!authorized) return Result::NoLicense;
        p += length;
    }
    return Result::Ok;
}

Result YakisobaBackend::processEcm(const std::shared_ptr<KeyRegistry::Slot>& slot, View payload) {
    if (!slot || payload.data == nullptr || payload.size < 30 || payload.size > 256) return Result::BadValue;
    std::unique_lock lock(mutex_, std::defer_lock);
    if (!lock.try_lock_for(kLockDeadline)) return Result::Busy;
    auto result = initialize();
    if (result != Result::Ok) return result;
    const int group = groupIndex(payload[1]);
    if (group < 0 || !protocolSupported(payload[0])) return Result::Unsupported;
    Secret<16> keys;
    result = decodeResult(bcas_decodeECM(payload.data, payload.size, keys.bytes.data(), nullptr));
    if (result != Result::Ok) return result;
    Secret<8> workKey;
    if (GetKey(payload[1], payload[2], workKey.bytes.data()) != 0) return failClosed();
    Secret<256> plain;
    std::copy_n(payload.data, 3, plain.bytes.data());
    Transform(payload[0], workKey.bytes.data(), payload.data + 3, payload.size - 3,
              plain.bytes.data() + 3, TRUE);
    result = validateEcm({plain.bytes.data(), payload.size}, state_.groups[static_cast<size_t>(group)]);
    if (result != Result::Ok) return result;
    return KeyRegistry::instance().update(slot, keys, payload[1], state_.groups[static_cast<size_t>(group)].expires);
}

Result YakisobaBackend::applyMessage(const EmmMessage& message) {
    const auto payload = message.payload;
    if (memcmp(payload.data, GetCardId(), 6) != 0) return Result::Ok;
    const auto protocol = payload[message.individual ? 8 : 7];
    if (!protocolSupported(protocol) && !(message.individual && protocol == 0xff)) return Result::Unsupported;
    Secret<256> output;
    const auto decoded = bcas_decodeEMM(payload.data, payload.size, output.bytes.data(), message.individual);
    if (decoded == -ENOMSG) return Result::Ok;
    auto result = decodeResult(decoded);
    if (result != Result::Ok) return result;
    if (message.individual) return Result::Unsupported;
    const auto* plain = output.bytes.data();
    if (memcmp(plain, GetCardId(), 6) || static_cast<size_t>(plain[6]) + 7 != payload.size) return Result::BadValue;
    const int group = groupIndex(plain[8]);
    if (group < 0) return Result::Unsupported;
    const auto groupIndexValue = static_cast<size_t>(group);
    const auto& entitlement = state_.groups[groupIndexValue];
    const auto number = be16(plain + 9);
    const auto expires = be16(plain + 11);
    if (todayMjd() > expires) return Result::Expired;
    if (entitlement.updated && number <= entitlement.number) {
        const bool duplicate = number == entitlement.number && entitlement.lastMessage.size() == payload.size &&
            std::equal(entitlement.lastMessage.begin(), entitlement.lastMessage.end(), payload.data);
        return duplicate ? Result::Ok : Result::InvalidState;
    }

    struct Update { uint8_t id; Secret<8> key; bool duplicate = false; };
    std::vector<Update> updates;
    std::array<bool, kYakisobaKeyBuckets> buckets{};
    auto bitmap = entitlement.bitmap;
    bool bitmapChanged = false;
    for (size_t p = 13; p < payload.size - 4;) {
        if (payload.size - 4 - p < 2) return Result::BadValue;
        const auto tag = plain[p++];
        const auto length = plain[p++];
        if (length > payload.size - 4 - p) return Result::BadValue;
        if (tag == 0x10) {
            if (length != 9 || plain[p] == 0xff || buckets[plain[p] % kYakisobaKeyBuckets]) return Result::BadValue;
            buckets[plain[p] % kYakisobaKeyBuckets] = true;
            Update update;
            update.id = plain[p];
            std::copy_n(plain + p + 1, 8, update.key.bytes.data());
            // libyakisobaのRegister()は即時更新かつ巻戻し不能なので、同じ受理条件を更新前に確認する。
            Secret<8> previous;
            for (unsigned id = update.id % kYakisobaKeyBuckets; id < 255; id += kYakisobaKeyBuckets) {
                if (GetKey(plain[8], static_cast<uint8_t>(id), previous.bytes.data()) != 0) continue;
                if (id > update.id) return Result::InvalidState;
                if (id == update.id) {
                    if (previous.bytes != update.key.bytes) return Result::InvalidState;
                    update.duplicate = true;
                }
            }
            updates.push_back(update);
        } else if (tag == 0x11) {
            if (length > 32 || bitmapChanged) return Result::BadValue;
            bitmap.fill(0);
            std::copy_n(plain + p, length, bitmap.data());
            bitmapChanged = true;
        } else {
            return Result::Unsupported;
        }
        p += length;
    }
    if (updates.empty() && !bitmapChanged) return Result::Unsupported;

    Bytes committedMessage(payload.data, payload.data + payload.size);
    YakisobaPersistentState next = state_;
    auto& nextEntitlement = next.groups[groupIndexValue];
    nextEntitlement.updated = true;
    nextEntitlement.number = number;
    nextEntitlement.expires = expires;
    nextEntitlement.bitmap = bitmap;
    nextEntitlement.lastMessage = committedMessage;
    for (const auto& update : updates) {
        auto& saved = nextEntitlement.keys[update.id % kYakisobaKeyBuckets];
        saved.present = true;
        saved.id = update.id;
        saved.key = update.key.bytes;
    }

    const auto committed = stateStore_.commit(next);
    if (committed == PersistentCommitResult::Unchanged) return Result::Unknown;
    if (committed == PersistentCommitResult::OutcomeUnknown) return failClosed();

    for (const auto& update : updates) {
        if (!update.duplicate && Register(plain[8], update.id, update.key.bytes.data()) != 0) return failClosed();
    }
    state_ = next;
    KeyRegistry::instance().invalidateGroup(plain[8]);
    return Result::Ok;
}

Result YakisobaBackend::processEmm(const std::vector<EmmMessage>& messages) {
    if (messages.empty()) return Result::BadValue;
    for (const auto& message : messages) {
        const auto payload = message.payload;
        const size_t minimum = message.individual ? 25 : 17;
        if (payload.data == nullptr || payload.size < minimum || payload.size > 256) return Result::BadValue;
        const size_t declared = message.individual ? 8 + be16(payload.data + 6) : 7 + payload[6];
        if (declared != payload.size) return Result::BadValue;
    }
    std::unique_lock lock(mutex_, std::defer_lock);
    if (!lock.try_lock_for(kLockDeadline)) return Result::Busy;
    auto result = initialize();
    if (result != Result::Ok) return result;
    for (const auto& message : messages) {
        result = applyMessage(message);
        if (result != Result::Ok) return result;
    }
    return Result::Ok;
}

Result YakisobaBackend::resolve(const Token& token, Secret<16>* keys) {
    if (keys == nullptr) return Result::BadValue;
    eraseSecret(keys->bytes.data(), keys->bytes.size());
    std::unique_lock lock(mutex_, std::defer_lock);
    if (!lock.try_lock_for(kLockDeadline)) return Result::Busy;
    if (revoked_) return Result::Revoked;
    if (!initialized_) return Result::NotProvisioned;
    return KeyRegistry::instance().resolve(token, keys);
}

}  // namespace maleicacid::cas
