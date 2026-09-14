// SPDX-License-Identifier: GPL-3.0-only
#include "SharedSlot.h"

#include <cstdio>
#include <cstring>
#include <fcntl.h>
#include <linux/memfd.h>
#include <new>
#include <sys/mman.h>
#include <sys/syscall.h>
#include <time.h>
#include <unistd.h>

namespace maleicacid::cas {

uint32_t todayMjd() {
    const auto now = time(nullptr);
    if (now < 0) return UINT32_MAX;
    const uint64_t days = static_cast<uint64_t>(now) / 86400 + 40587;
    return days > UINT32_MAX ? UINT32_MAX : static_cast<uint32_t>(days);
}

bool SharedKeyState::store(const Secret<16>* value, uint32_t expiry) {
    // writer は KeyRegistry の mutex で直列化済み。全 atomic を同じ順序で観測する。
    const auto before = sequence.load();
    if (before >= UINT64_MAX - 1) {
        sequence.store(UINT64_MAX);
        valid.store(0);
        for (auto& word : keys) word.store(0);
        return false;
    }
    sequence.store(before + 1);
    valid.store(value != nullptr);
    expires.store(expiry);
    for (size_t i = 0; i < keys.size(); ++i) {
        uint32_t word = 0;
        if (value) std::memcpy(&word, value->bytes.data() + i * sizeof(word), sizeof(word));
        keys[i].store(word);
        eraseSecret(&word, sizeof(word));
    }
    sequence.store(before + 2);
    return true;
}

Result SharedKeyState::snapshot(Secret<16>* value) const {
    eraseSecret(value->bytes.data(), value->bytes.size());
    // writer が停止・終了した場合も無期限に待たない。
    for (int attempt = 0; attempt < 4; ++attempt) {
        const auto before = sequence.load();
        if (before == UINT64_MAX) return Result::SessionClosed;
        if (before & 1) continue;
        const auto ready = valid.load();
        const auto expiry = expires.load();
        Secret<16> current;
        for (size_t i = 0; i < keys.size(); ++i) {
            auto word = keys[i].load();
            std::memcpy(current.bytes.data() + i * sizeof(word), &word, sizeof(word));
            eraseSecret(&word, sizeof(word));
        }
        if (sequence.load() != before) continue;
        if (!ready) return Result::NoLicense;
        if (todayMjd() > expiry) return Result::Expired;
        *value = current;
        return Result::Ok;
    }
    return Result::Busy;
}

std::unique_ptr<SharedSlot> SharedSlot::create() {
    const int fd = syscall(SYS_memfd_create, "maleicacid.cas.slot", MFD_CLOEXEC | MFD_ALLOW_SEALING);
    if (fd < 0) return {};
    struct Fd { int value; ~Fd() { if (value >= 0) close(value); } } cleanup{fd};
    if (ftruncate(fd, sizeof(SharedKeyState))) return {};
    void* memory = mmap(nullptr, sizeof(SharedKeyState), PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    if (memory == MAP_FAILED) return {};
    auto slot = std::unique_ptr<SharedSlot>(new (std::nothrow) SharedSlot(fd, new (memory) SharedKeyState{}));
    if (!slot) {
        munmap(memory, sizeof(SharedKeyState));
        return {};
    }
    cleanup.value = -1;
    // CAS の既存書込み mapping だけを残し、consumer の再 mapping / write を禁止する。
    if (fcntl(fd, F_ADD_SEALS, F_SEAL_GROW | F_SEAL_SHRINK | F_SEAL_FUTURE_WRITE | F_SEAL_SEAL)) {
        return {};
    }
    return slot;
}

SharedSlot::~SharedSlot() {
    state_->store(nullptr, 0);
    munmap(state_, sizeof(SharedKeyState));
    close(fd_);
}

int SharedSlot::readerFd() const {
    char path[64];
    snprintf(path, sizeof(path), "/proc/self/fd/%d", fd_);
    return open(path, O_RDONLY | O_CLOEXEC);
}

}  // namespace maleicacid::cas
