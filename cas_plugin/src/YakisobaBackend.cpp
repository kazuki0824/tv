// SPDX-License-Identifier: GPL-3.0-only
#include "YakisobaBackend.h"

#include <algorithm>
#include <cerrno>
#include <chrono>
#include <cstdio>
#include <cstring>
#include <fcntl.h>
#include <unistd.h>

extern "C" {
#include <Global.h>
#include <Crypto.h>
#include <Keyset.h>
#include <yakisoba.h>
// 採用する libyakisoba-cross の Keyset.c 内部関数。静的リンクの外へ公開しない。
int32_t Register(uint8_t group, uint8_t id, const uint8_t* key);
FILE* __real_fopen(const char* path, const char* mode);
}

namespace {

// 無改変の Keyset.c に、検証済みで長さの限定された初期入力だけを読ませる。
// --wrap=fopen はこの .so の静的依存に限って働き、他の DSO の I/O は変更しない。
thread_local char* initialInput = nullptr;
thread_local size_t initialSize = 0;
thread_local bool initialReadFailed = false;

}  // namespace

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

constexpr std::array<uint8_t, 6> kGroups{0x02, 0x03, 0x17, 0x1d, 0x1e, 0x20};
constexpr size_t kMaxCredentialSize = 16384;
constexpr auto kLockDeadline = std::chrono::milliseconds(500);

int groupIndex(uint8_t group) {
    auto it = std::find(kGroups.begin(), kGroups.end(), group);
    return it == kGroups.end() ? -1 : static_cast<int>(it - kGroups.begin());
}

bool protocolSupported(uint8_t protocol) {
    return (protocol & ~0x4c) == 0;
}

Result decodeResult(int result) {
    switch (result) {
        case 0: return Result::Ok;
        case -EINVAL: return Result::BadValue;
        case -EILSEQ: return Result::Decrypt;
        case -ENOKEY: return Result::NoLicense;
        default: return Result::Unknown;
    }
}

bool sameFile(const struct stat& a, const struct stat& b) {
    return a.st_dev == b.st_dev && a.st_ino == b.st_ino && a.st_size == b.st_size &&
           a.st_mode == b.st_mode && a.st_uid == b.st_uid && a.st_gid == b.st_gid &&
           a.st_mtim.tv_sec == b.st_mtim.tv_sec && a.st_mtim.tv_nsec == b.st_mtim.tv_nsec &&
           a.st_ctim.tv_sec == b.st_ctim.tv_sec && a.st_ctim.tv_nsec == b.st_ctim.tv_nsec;
}

bool trustedFile(const struct stat& info) {
#ifdef MALEICACID_CAS_TEST
    const uid_t owner = geteuid();
#else
    constexpr uid_t owner = 0;
#endif
    return S_ISREG(info.st_mode) && info.st_uid == owner &&
           (info.st_mode & 0027) == 0 && info.st_size > 0 &&
           info.st_size <= static_cast<off_t>(kMaxCredentialSize);
}

int hexDigit(uint8_t c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

// upstream が寛容に読み飛ばす入力を、初期化前に全行検証する。
// 部分的な CardKey や同じ台帳 bucket の競合を許可しない。
bool validCredentials(View input) {
    bool cardId = false;
    bool cardKey = false;
    std::array<std::array<bool, 10>, 6> buckets{};
    size_t offset = 0;
    while (offset < input.size) {
        size_t end = offset;
        while (end < input.size && input[end] != '\n') ++end;
        size_t p = offset;
        auto spaces = [&] { while (p < end && (input[p] == ' ' || input[p] == '\t' || input[p] == '\r')) ++p; };
        auto literal = [&](const char* text) {
            const size_t n = strlen(text);
            if (end - p < n || memcmp(input.data + p, text, n)) return false;
            p += n;
            return true;
        };
        auto byte = [&](uint8_t* value) {
            spaces();
            if (end - p < 2) return false;
            const int hi = hexDigit(input[p]);
            const int lo = hexDigit(input[p + 1]);
            if (hi < 0 || lo < 0) return false;
            *value = static_cast<uint8_t>((hi << 4) | lo);
            p += 2;
            return true;
        };
        spaces();
        if (p == end || input[p] == '#' || input[p] == ';') { offset = end + 1; continue; }
        if (literal("CardID")) {
            if (cardId) return false;
            cardId = true;
        } else if (literal("CardKey")) {
            if (cardKey) return false;
            cardKey = true;
        } else if (literal("Key")) {
            uint8_t group, id;
            spaces();
            if (!literal("[") || !byte(&group)) return false;
            spaces();
            if (!literal("]")) return false;
            spaces();
            if (!literal("[") || !byte(&id)) return false;
            spaces();
            if (!literal("]") || id == 0xff) return false;
            const int index = groupIndex(group);
            if (index < 0 || buckets[index][id % 10]) return false;
            buckets[index][id % 10] = true;
        } else {
            return false;
        }
        spaces();
        if (!literal("=")) return false;
        uint8_t ignored;
        for (int i = 0; i < 8; ++i) {
            if (!byte(&ignored)) return false;
            if (p < end && input[p] != ',' && input[p] != ' ' && input[p] != '\t' && input[p] != '\r') {
                return false;
            }
            if (p < end && input[p] == ',') ++p;
        }
        eraseSecret(&ignored, sizeof(ignored));
        spaces();
        if (p != end) return false;
        offset = end + 1;
    }
    return cardId && cardKey;
}

}  // namespace

YakisobaBackend& YakisobaBackend::instance() {
    static YakisobaBackend owner;
    return owner;
}

#ifdef MALEICACID_CAS_TEST
void YakisobaBackend::setCredentialPathForTest(std::string path) {
    std::lock_guard lock(mutex_);
    if (!initialized_) credentialPath_ = std::move(path);
}
#endif

Result YakisobaBackend::failClosed() {
    revoked_ = true;
    KeyRegistry::instance().revokeAll();
    return Result::Revoked;
}

Result YakisobaBackend::checkCredential() {
    if (revoked_) return Result::Revoked;
    if (!initialized_) return Result::NotProvisioned;
    struct stat info {};
    if (lstat(credentialPath_.c_str(), &info) || !sameFile(info, credentialStat_)) {
        return failClosed();
    }
    return Result::Ok;
}

Result YakisobaBackend::initialize() {
    if (initialized_ || revoked_) return checkCredential();
    const int fd = open(credentialPath_.c_str(), O_RDONLY | O_CLOEXEC | O_NOFOLLOW | O_NONBLOCK);
    if (fd < 0) return Result::NotProvisioned;
    struct stat before {}, after {};
    if (fstat(fd, &before) || !trustedFile(before)) { close(fd); return Result::NotProvisioned; }
    Secret<kMaxCredentialSize + 1> content;
    const auto size = static_cast<size_t>(before.st_size);
    const auto count = pread(fd, content.bytes.data(), size + 1, 0);
    const bool stable = fstat(fd, &after) == 0 && sameFile(before, after);
    close(fd);
    if (count != static_cast<ssize_t>(size) || !stable ||
        !validCredentials({content.bytes.data(), size})) return Result::NotProvisioned;
    initialInput = reinterpret_cast<char*>(content.bytes.data());
    initialSize = size;
    initialReadFailed = false;
    // InitKeys → 初期 credential → Register の順序を、全 plugin 共通 lock 内で固定する。
    GetCardId();
    initialInput = nullptr;
    initialSize = 0;
    if (initialReadFailed) return failClosed();
    credentialStat_ = before;
    initialized_ = true;
    return checkCredential();
}

Result YakisobaBackend::validateEcm(View plain, const Entitlement& entitlement) {
    if (todayMjd() > entitlement.expires) return Result::Expired;
    // 有料・無料の通常番組だけを扱う。未知の権利判定を復号成功へ丸めない。
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
    std::unique_lock lock(mutex_, std::defer_lock);
    if (!lock.try_lock_for(kLockDeadline)) return Result::Busy;
    auto result = initialize();
    if (result != Result::Ok) return result;
    if (payload.size < 30 || payload.size > 256) return Result::BadValue;
    const int group = groupIndex(payload[1]);
    if (group < 0 || !protocolSupported(payload[0])) return Result::Unsupported;
    Secret<16> keys;
    result = decodeResult(bcas_decodeECM(payload.data, payload.size, keys.bytes.data(), nullptr));
    if (result != Result::Ok) return result;
    Secret<8> workKey;
    if (GetKey(payload[1], payload[2], workKey.bytes.data()) != 0) return failClosed();
    Secret<256> plain;
    std::copy_n(payload.data, 3, plain.bytes.data());
    // decode API の公開出力にない固定権利情報も、MAC 検証後に検査する。
    Transform(payload[0], workKey.bytes.data(), payload.data + 3, payload.size - 3,
              plain.bytes.data() + 3, TRUE);
    result = validateEcm({plain.bytes.data(), payload.size}, entitlements_[group]);
    if (result != Result::Ok) return result;
    result = checkCredential();
    if (result != Result::Ok) return result;
    return KeyRegistry::instance().update(slot, keys, payload[1], entitlements_[group].expires);
}

Result YakisobaBackend::applyMessage(const EmmMessage& message) {
    const auto payload = message.payload;
    if (memcmp(payload.data, GetCardId(), 6) != 0) return Result::Ok;
    const auto protocol = payload[message.individual ? 8 : 7];
    if (!protocolSupported(protocol) && !(message.individual && protocol == 0xff)) {
        return Result::Unsupported;
    }
    Secret<256> output;
    const auto decoded = bcas_decodeEMM(payload.data, payload.size, output.bytes.data(), message.individual);
    if (decoded == -ENOMSG) return Result::Ok;
    auto result = decodeResult(decoded);
    if (result != Result::Ok) return result;
    // 個別表示メッセージには work key 更新の意味がない。未実装の表示を成功にしない。
    if (message.individual) return Result::Unsupported;
    const auto* plain = output.bytes.data();
    if (memcmp(plain, GetCardId(), 6) || static_cast<size_t>(plain[6]) + 7 != payload.size) {
        return Result::BadValue;
    }
    const int group = groupIndex(plain[8]);
    if (group < 0) return Result::Unsupported;
    auto& entitlement = entitlements_[group];
    const auto number = be16(plain + 9);
    const auto expires = be16(plain + 11);
    if (todayMjd() > expires) return Result::Expired;
    if (entitlement.updated && number <= entitlement.number) {
        const bool duplicate = number == entitlement.number &&
            entitlement.lastMessage.size() == payload.size &&
            std::equal(entitlement.lastMessage.begin(), entitlement.lastMessage.end(), payload.data);
        return duplicate ? Result::Ok : Result::InvalidState;
    }

    struct Update { uint8_t id; Secret<8> key; bool duplicate = false; };
    std::vector<Update> updates;
    std::array<bool, 10> buckets{};
    auto bitmap = entitlement.bitmap;
    bool bitmapChanged = false;
    for (size_t p = 13; p < payload.size - 4;) {
        if (payload.size - 4 - p < 2) return Result::BadValue;
        const auto tag = plain[p++];
        const auto length = plain[p++];
        if (length > payload.size - 4 - p) return Result::BadValue;
        if (tag == 0x10) {
            if (length != 9 || plain[p] == 0xff || buckets[plain[p] % 10]) return Result::BadValue;
            buckets[plain[p] % 10] = true;
            Update update;
            update.id = plain[p];
            std::copy_n(plain + p + 1, 8, update.key.bytes.data());
            // 内部 Register の同一 bucket 拒否を、書込み前に台帳自身で検証する。
            Secret<8> previous;
            for (unsigned id = update.id % 10; id < 255; id += 10) {
                if (GetKey(plain[8], id, previous.bytes.data()) != 0) continue;
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
    // 確定開始後に allocation が失敗しないよう、再配送識別用の暗号文を先に確保する。
    Bytes committedMessage(payload.data, payload.data + payload.size);
    result = checkCredential();
    if (result != Result::Ok) return result;
    for (const auto& update : updates) {
        if (!update.duplicate && Register(plain[8], update.id, update.key.bytes.data()) != 0) {
            return failClosed();
        }
    }
    entitlement.updated = true;
    entitlement.number = number;
    entitlement.expires = expires;
    entitlement.bitmap = bitmap;
    entitlement.lastMessage = std::move(committedMessage);
    // 更新前の権利で導出した Ks を次の packet へ再取得させない。
    KeyRegistry::instance().invalidateGroup(plain[8]);
    return Result::Ok;
}

Result YakisobaBackend::processEmm(const std::vector<EmmMessage>& messages) {
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
    const auto result = checkCredential();
    return result == Result::Ok ? KeyRegistry::instance().resolve(token, keys) : result;
}

}  // namespace maleicacid::cas
