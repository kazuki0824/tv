// SPDX-License-Identifier: GPL-3.0-only
#include "YakisobaState.h"

#include <algorithm>
#include <array>
#include <cerrno>
#include <cstring>
#include <fcntl.h>
#include <limits>
#include <string>
#include <sys/stat.h>
#include <unistd.h>

namespace maleicacid::cas {
namespace {
constexpr std::array<uint8_t, 8> kMagic{'M','C','A','S','P','S','0','1'};
constexpr size_t kMaxStateSize = 16384;

void put16(Bytes* out, uint16_t value) {
    out->push_back(static_cast<uint8_t>(value >> 8));
    out->push_back(static_cast<uint8_t>(value));
}
void put32(Bytes* out, uint32_t value) {
    out->push_back(static_cast<uint8_t>(value >> 24));
    out->push_back(static_cast<uint8_t>(value >> 16));
    out->push_back(static_cast<uint8_t>(value >> 8));
    out->push_back(static_cast<uint8_t>(value));
}
bool take8(const Bytes& in, size_t* p, uint8_t* value) {
    if (*p >= in.size()) return false;
    *value = in[(*p)++];
    return true;
}
bool take16(const Bytes& in, size_t* p, uint16_t* value) {
    if (in.size() - *p < 2) return false;
    *value = static_cast<uint16_t>((static_cast<uint16_t>(in[*p]) << 8) | in[*p + 1]);
    *p += 2;
    return true;
}
bool take32(const Bytes& in, size_t* p, uint32_t* value) {
    if (in.size() - *p < 4) return false;
    *value = (static_cast<uint32_t>(in[*p]) << 24) |
             (static_cast<uint32_t>(in[*p + 1]) << 16) |
             (static_cast<uint32_t>(in[*p + 2]) << 8) |
             static_cast<uint32_t>(in[*p + 3]);
    *p += 4;
    return true;
}
bool appendBytes(Bytes* out, const uint8_t* data, size_t size) {
    if (size > kMaxStateSize || out->size() > kMaxStateSize - size) return false;
    out->insert(out->end(), data, data + size);
    return true;
}
bool takeBytes(const Bytes& in, size_t* p, uint8_t* out, size_t size) {
    if (size > in.size() - *p) return false;
    std::copy_n(in.data() + *p, size, out);
    *p += size;
    return true;
}
std::string parentPath(const std::string& path) {
    const auto slash = path.rfind('/');
    if (slash == std::string::npos) return ".";
    if (slash == 0) return "/";
    return path.substr(0, slash);
}
bool writeAll(int fd, const uint8_t* data, size_t size) {
    while (size != 0) {
        const auto n = write(fd, data, size);
        if (n < 0) {
            if (errno == EINTR) continue;
            return false;
        }
        if (n == 0) return false;
        data += static_cast<size_t>(n);
        size -= static_cast<size_t>(n);
    }
    return true;
}
}

YakisobaPersistentState::~YakisobaPersistentState() {
    for (auto& group : groups) {
        for (auto& key : group.keys) eraseSecret(key.key.data(), key.key.size());
    }
}

PersistentLoadResult YakisobaStateStore::load(const std::array<uint8_t, 6>& cardId,
                                               YakisobaPersistentState* state) const {
    if (state == nullptr) return PersistentLoadResult::Error;
    const int fd = open(path_.c_str(), O_RDONLY | O_CLOEXEC | O_NOFOLLOW);
    if (fd < 0) return errno == ENOENT ? PersistentLoadResult::Missing : PersistentLoadResult::Error;
    struct stat info {};
    if (fstat(fd, &info) != 0 || !S_ISREG(info.st_mode) || info.st_size <= 0 ||
        info.st_size > static_cast<off_t>(kMaxStateSize)) {
        close(fd);
        return PersistentLoadResult::Error;
    }
    Bytes input(static_cast<size_t>(info.st_size));
    size_t offset = 0;
    while (offset < input.size()) {
        const auto n = read(fd, input.data() + offset, input.size() - offset);
        if (n < 0) {
            if (errno == EINTR) continue;
            close(fd);
            return PersistentLoadResult::Error;
        }
        if (n == 0) break;
        offset += static_cast<size_t>(n);
    }
    close(fd);
    if (offset != input.size()) return PersistentLoadResult::Error;

    YakisobaPersistentState parsed;
    size_t p = 0;
    std::array<uint8_t, kMagic.size()> magic{};
    if (!takeBytes(input, &p, magic.data(), magic.size()) || magic != kMagic ||
        !takeBytes(input, &p, parsed.cardId.data(), parsed.cardId.size()) || parsed.cardId != cardId) {
        return PersistentLoadResult::Error;
    }
    for (size_t g = 0; g < kYakisobaGroups.size(); ++g) {
        uint8_t group = 0, updated = 0;
        auto& entry = parsed.groups[g];
        if (!take8(input, &p, &group) || group != kYakisobaGroups[g] ||
            !take8(input, &p, &updated) || updated > 1 ||
            !take16(input, &p, &entry.number) || !take32(input, &p, &entry.expires) ||
            !takeBytes(input, &p, entry.bitmap.data(), entry.bitmap.size())) {
            return PersistentLoadResult::Error;
        }
        entry.updated = updated != 0;
        uint16_t messageSize = 0;
        if (!take16(input, &p, &messageSize) || messageSize > 256 || input.size() - p < messageSize) {
            return PersistentLoadResult::Error;
        }
        entry.lastMessage.assign(input.begin() + static_cast<ptrdiff_t>(p),
                                 input.begin() + static_cast<ptrdiff_t>(p + messageSize));
        p += messageSize;
        for (size_t bucket = 0; bucket < kYakisobaKeyBuckets; ++bucket) {
            uint8_t present = 0;
            if (!take8(input, &p, &present) || present > 1) return PersistentLoadResult::Error;
            if (!present) continue;
            auto& key = entry.keys[bucket];
            key.present = true;
            if (!take8(input, &p, &key.id) || key.id == 0xff || key.id % kYakisobaKeyBuckets != bucket ||
                !takeBytes(input, &p, key.key.data(), key.key.size())) {
                return PersistentLoadResult::Error;
            }
        }
    }
    if (p != input.size()) return PersistentLoadResult::Error;
    *state = parsed;
    return PersistentLoadResult::Ok;
}

PersistentCommitResult YakisobaStateStore::commit(const YakisobaPersistentState& state) const {
    Bytes output;
    output.reserve(4096);
    if (!appendBytes(&output, kMagic.data(), kMagic.size()) ||
        !appendBytes(&output, state.cardId.data(), state.cardId.size())) return PersistentCommitResult::Unchanged;
    for (size_t g = 0; g < kYakisobaGroups.size(); ++g) {
        const auto& entry = state.groups[g];
        output.push_back(kYakisobaGroups[g]);
        output.push_back(entry.updated ? 1 : 0);
        put16(&output, entry.number);
        put32(&output, entry.expires);
        if (!appendBytes(&output, entry.bitmap.data(), entry.bitmap.size()) || entry.lastMessage.size() > 256) {
            return PersistentCommitResult::Unchanged;
        }
        put16(&output, static_cast<uint16_t>(entry.lastMessage.size()));
        if (!appendBytes(&output, entry.lastMessage.data(), entry.lastMessage.size())) return PersistentCommitResult::Unchanged;
        for (size_t bucket = 0; bucket < kYakisobaKeyBuckets; ++bucket) {
            const auto& key = entry.keys[bucket];
            output.push_back(key.present ? 1 : 0);
            if (!key.present) continue;
            if (key.id == 0xff || key.id % kYakisobaKeyBuckets != bucket) return PersistentCommitResult::Unchanged;
            output.push_back(key.id);
            if (!appendBytes(&output, key.key.data(), key.key.size())) return PersistentCommitResult::Unchanged;
        }
    }
    if (output.size() > kMaxStateSize) return PersistentCommitResult::Unchanged;

    const std::string temp = path_ + ".tmp." + std::to_string(static_cast<long long>(getpid()));
    const int fd = open(temp.c_str(), O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC | O_NOFOLLOW, 0600);
    if (fd < 0) return PersistentCommitResult::Unchanged;
    bool ok = writeAll(fd, output.data(), output.size()) && fsync(fd) == 0;
    if (close(fd) != 0) ok = false;
    if (!ok || rename(temp.c_str(), path_.c_str()) != 0) {
        unlink(temp.c_str());
        return PersistentCommitResult::Unchanged;
    }
    const auto parent = parentPath(path_);
    const int dir = open(parent.c_str(), O_RDONLY | O_CLOEXEC | O_DIRECTORY);
    if (dir < 0) return PersistentCommitResult::OutcomeUnknown;
    ok = fsync(dir) == 0;
    if (close(dir) != 0) ok = false;
    return ok ? PersistentCommitResult::Ok : PersistentCommitResult::OutcomeUnknown;
}

}  // namespace maleicacid::cas
