// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include "Types.h"
#include <array>
#include <cstdint>
#include <string>

namespace maleicacid::cas {

constexpr std::array<uint8_t, 6> kYakisobaGroups{0x02, 0x03, 0x17, 0x1d, 0x1e, 0x20};
constexpr size_t kYakisobaKeyBuckets = 10;

struct PersistedWorkKey {
    bool present = false;
    uint8_t id = 0xff;
    std::array<uint8_t, 8> key{};
};

struct PersistedWorkKeyGroup {
    bool updated = false;
    uint16_t number = 0;
    Bytes lastMessage;
    std::array<PersistedWorkKey, kYakisobaKeyBuckets> keys{};
};

struct YakisobaPersistentState {
    std::array<uint8_t, 6> cardId{};
    std::array<PersistedWorkKeyGroup, kYakisobaGroups.size()> groups{};
    ~YakisobaPersistentState();
};

enum class PersistentLoadResult { Missing, Ok, Error };
enum class PersistentCommitResult { Ok, Unchanged, OutcomeUnknown };

class YakisobaStateStore {
public:
    explicit YakisobaStateStore(std::string path = "/data/vendor/maleicacid/cas/yakisoba_state")
        : path_(std::move(path)) {}
    PersistentLoadResult load(const std::array<uint8_t, 6>& cardId, YakisobaPersistentState* state) const;
    PersistentCommitResult commit(const YakisobaPersistentState& state) const;
#ifdef MALEICACID_CAS_TEST
    void setPathForTest(std::string path) { path_ = std::move(path); }
#endif
private:
    std::string path_;
};

}  // namespace maleicacid::cas
