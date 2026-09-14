// SPDX-License-Identifier: GPL-3.0-only
#pragma once

#include "Types.h"

#include <atomic>
#include <memory>

namespace maleicacid::cas {

// 同じ製品にリンクする CAS / Tuner 間の内部 ABI。CAS だけが書き込む。
// payload も atomic とし、更新と読取りの競合で C++ の data race を起こさない。
struct SharedKeyState {
    std::atomic<uint64_t> sequence{0};
    std::atomic<uint32_t> valid{0};
    std::atomic<uint32_t> expires{0};
    std::array<std::atomic<uint32_t>, 4> keys{};

    bool store(const Secret<16>* value, uint32_t expiry);
    Result snapshot(Secret<16>* value) const;
};

static_assert(std::atomic<uint64_t>::is_always_lock_free);
static_assert(std::atomic<uint32_t>::is_always_lock_free);

class SharedSlot {
public:
    static std::unique_ptr<SharedSlot> create();
    ~SharedSlot();
    SharedKeyState& state() { return *state_; }
    // 呼出し元が所有する読取り専用 fd。失敗時は -1。
    int readerFd() const;
private:
    SharedSlot(int fd, SharedKeyState* state) : fd_(fd), state_(state) {}
    int fd_;
    SharedKeyState* state_;
};

}  // namespace maleicacid::cas
