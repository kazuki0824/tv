// SPDX-License-Identifier: GPL-3.0-only
#include "YakisobaState.h"

#include <cstdlib>
#include <fcntl.h>
#include <stdexcept>
#include <string>
#include <unistd.h>

namespace {
using namespace maleicacid::cas;

#define CHECK(expression) do { if (!(expression)) throw std::runtime_error( \
    std::string(__FILE__) + ":" + std::to_string(__LINE__) + ": " #expression); } while (false)

void writeByte(const std::string& path, uint8_t value) {
    const int fd = open(path.c_str(), O_WRONLY | O_APPEND | O_CLOEXEC);
    CHECK(fd >= 0);
    CHECK(write(fd, &value, 1) == 1);
    CHECK(close(fd) == 0);
}

void roundTripAndRejectCorruption() {
    const char* base = access("/data/local/tmp", W_OK) == 0 ? "/data/local/tmp" : "/tmp";
    std::string patternText = std::string(base) + "/maleicacid-yakisoba-state-XXXXXX";
    std::vector<char> pattern(patternText.begin(), patternText.end());
    pattern.push_back(0);
    char* directory = mkdtemp(pattern.data());
    CHECK(directory != nullptr);
    const std::string path = std::string(directory) + "/yakisoba_state";
    YakisobaStateStore store(path);

    YakisobaPersistentState state;
    state.cardId = {1, 2, 3, 4, 5, 6};
    CHECK(store.load(state.cardId, &state) == PersistentLoadResult::Missing);

    auto& group = state.groups[0];
    group.updated = true;
    group.number = 7;
    group.lastMessage = {1, 2, 3};
    auto& key = group.keys[1];
    key.present = true;
    key.id = 11;
    key.key = {1, 2, 3, 4, 5, 6, 7, 8};

    CHECK(store.commit(state) == PersistentCommitResult::Ok);
    YakisobaPersistentState restored;
    CHECK(store.load(state.cardId, &restored) == PersistentLoadResult::Ok);
    CHECK(restored.groups[0].number == 7);
    CHECK(restored.groups[0].lastMessage == Bytes({1, 2, 3}));
    CHECK(restored.groups[0].keys[1].present);
    CHECK(restored.groups[0].keys[1].id == 11);
    const std::array<uint8_t, 8> expectedKey{1, 2, 3, 4, 5, 6, 7, 8};
    CHECK(restored.groups[0].keys[1].key == expectedKey);

    auto wrongCard = state.cardId;
    ++wrongCard[0];
    CHECK(store.load(wrongCard, &restored) == PersistentLoadResult::Error);

    writeByte(path, 0xff);
    CHECK(store.load(state.cardId, &restored) == PersistentLoadResult::Error);

    CHECK(unlink(path.c_str()) == 0);
    CHECK(rmdir(directory) == 0);
}
}  // namespace

int main() {
    roundTripAndRejectCorruption();
    return 0;
}
