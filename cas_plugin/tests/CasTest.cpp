// SPDX-License-Identifier: GPL-3.0-only
#include <media/cas/CasAPI.h>
#include <media/stagefright/MediaErrors.h>
#include <maleicacid/cas/KeyClient.h>
#include <maleicacid/cas/KeyClientC.h>

#include "KeyRegistry.h"
#include "KeySocket.h"
#include "Section.h"
#include "YakisobaBackend.h"

#include <algorithm>
#include <atomic>
#include <csignal>
#include <chrono>
#include <cstdio>
#include <cstring>
#include <fcntl.h>
#include <functional>
#include <memory>
#include <poll.h>
#include <stdexcept>
#include <sys/stat.h>
#include <sys/wait.h>
#include <sys/mman.h>
#include <thread>
#include <unistd.h>

#ifdef __ANDROID__
#include <gtest/gtest.h>
#endif

extern "C" {
#include <Global.h>
#include <Crypto.h>
android::CasFactory* createCasFactory();
}

namespace {
using namespace maleicacid::cas;
using namespace android;

#define CHECK(expression) do { if (!(expression)) throw std::runtime_error( \
    std::string(__FILE__) + ":" + std::to_string(__LINE__) + ": " #expression); } while (false)

constexpr std::array<uint8_t, 8> kCard{1, 2, 3, 4, 5, 6, 7, 8};
constexpr std::array<uint8_t, 8> kCardKey{0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17};
constexpr std::array<uint8_t, 8> kWorkKey{0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37};
constexpr std::array<uint8_t, 8> kNextWorkKey{0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47};
constexpr std::array<uint8_t, 16> kKeys{
    0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7,
    0xb0, 0xb1, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7,
};

Bytes section(uint8_t table, const Bytes& payload, uint16_t extension = 0) {
    const auto size = payload.size() + 9;
    Bytes result{table, static_cast<uint8_t>(0xf0 | (size >> 8)), static_cast<uint8_t>(size),
                 static_cast<uint8_t>(extension >> 8), static_cast<uint8_t>(extension), 0xc1, 0, 0};
    result.insert(result.end(), payload.begin(), payload.end());
    const auto crc = sectionCrc({result.data(), result.size()});
    for (int shift : {24, 16, 8, 0}) result.push_back(static_cast<uint8_t>(crc >> shift));
    return result;
}

Bytes ecm(uint8_t id = 1, std::array<uint8_t, 8> work = kWorkKey,
          std::array<uint8_t, 16> keys = kKeys, Bytes commands = {},
          uint8_t program = 0, uint8_t protocol = 0x40) {
    Bytes plain(26, 0);
    plain[0] = protocol;
    plain[1] = 2;
    plain[2] = id;
    std::copy(keys.begin(), keys.end(), plain.begin() + 3);
    plain[19] = program;
    plain[20] = static_cast<uint8_t>(todayMjd() >> 8);
    plain[21] = static_cast<uint8_t>(todayMjd());
    plain[22] = 0x12;
    plain.insert(plain.end(), commands.begin(), commands.end());
    const auto length = plain.size();
    plain.resize(length + 4);
    GenerateMAC(protocol, work.data(), plain.data(), length, plain.data() + length);
    Bytes encrypted(plain.size());
    std::copy_n(plain.begin(), 3, encrypted.begin());
    Transform(protocol, work.data(), plain.data() + 3, plain.size() - 3,
              encrypted.data() + 3, FALSE);
    return section(0x82, encrypted);
}

Bytes updateKey(uint8_t id, const std::array<uint8_t, 8>& key) {
    Bytes result{0x10, 9, id};
    result.insert(result.end(), key.begin(), key.end());
    return result;
}

Bytes emmPayload(uint16_t number, Bytes commands, uint16_t expires = 0xffff,
                 std::array<uint8_t, 8> card = kCard) {
    Bytes plain(13);
    std::copy_n(card.begin(), 6, plain.begin());
    plain[6] = static_cast<uint8_t>(commands.size() + 10);
    plain[7] = 0x40;
    plain[8] = 2;
    plain[9] = static_cast<uint8_t>(number >> 8);
    plain[10] = static_cast<uint8_t>(number);
    plain[11] = static_cast<uint8_t>(expires >> 8);
    plain[12] = static_cast<uint8_t>(expires);
    plain.insert(plain.end(), commands.begin(), commands.end());
    const auto length = plain.size();
    plain.resize(length + 4);
    GenerateMAC(0x40, kCardKey.data(), plain.data(), length, plain.data() + length);
    Bytes encrypted(plain.size());
    std::copy_n(plain.begin(), 8, encrypted.begin());
    Transform(0x40, kCardKey.data(), plain.data() + 8, plain.size() - 8,
              encrypted.data() + 8, FALSE);
    return encrypted;
}

Bytes individualPayload() {
    Bytes plain(25);
    std::copy_n(kCard.begin(), 6, plain.begin());
    plain[7] = 17;
    plain[8] = 0x40;
    plain[9] = 2;
    plain[14] = 0xff;
    plain[15] = 0xff;
    GenerateMAC(0x40, kCardKey.data(), plain.data() + 12, 9, plain.data() + 21);
    Bytes encrypted(plain.size());
    std::copy_n(plain.begin(), 12, encrypted.begin());
    Transform(0x40, kCardKey.data(), plain.data() + 12, 13, encrypted.data() + 12, FALSE);
    return encrypted;
}

class Environment {
public:
    Environment() {
        const char* directory = access("/data/local/tmp", W_OK) == 0 ? "/data/local/tmp" : "/tmp";
        std::string name = std::string(directory) + "/maleicacid-bcas-XXXXXX";
        std::vector<char> pattern(name.begin(), name.end());
        pattern.push_back(0);
        const int fd = mkstemp(pattern.data());
        CHECK(fd >= 0);
        path = pattern.data();
        const std::string credential =
            "CardID = 01 02 03 04 05 06 07 08\n"
            "CardKey = 10 11 12 13 14 15 16 17\n"
            "Key [02] [01] = 30 31 32 33 34 35 36 37\n";
        CHECK(write(fd, credential.data(), credential.size()) == static_cast<ssize_t>(credential.size()));
        CHECK(close(fd) == 0);
        YakisobaBackend::instance().setCredentialPathForTest(path);
        factory.reset(::createCasFactory());
        CHECK(factory);
    }
    ~Environment() { unlink(path.c_str()); }
    std::unique_ptr<CasPlugin> plugin(void* data = nullptr, bool legacy = false) {
        CasPlugin* plugin = nullptr;
        auto result = legacy
            ? factory->createPlugin(kB25SystemId, data, static_cast<CasPluginCallback>(nullptr), &plugin)
            : factory->createPlugin(kB25SystemId, data, static_cast<CasPluginCallbackExt>(nullptr), &plugin);
        CHECK(result == OK);
        CHECK(plugin != nullptr);
        return std::unique_ptr<CasPlugin>(plugin);
    }
    std::string path;
    std::unique_ptr<CasFactory> factory;
};

CasSessionId open(CasPlugin& plugin) {
    CasSessionId id;
    CHECK(plugin.openSession(&id) == OK);
    CHECK(id.size() == 16);
    return id;
}

// 単発の照会を使う既存 ABI/入力試験用。製品の packet 経路は bind 済み参照を使う。
KeyResult acquirePacketKeys(const uint8_t* token, size_t size, PacketKeys* keys) {
    if (keys == nullptr) return KeyResult::InvalidToken;
    *keys = PacketKeys{};
    std::unique_ptr<KeyReference> reference;
    const auto result = KeyReference::bind(token, size, &reference);
    return result == KeyResult::Ok ? reference->snapshot(keys) : result;
}

bool resolve(const CasSessionId& id, std::array<uint8_t, 16>* result = nullptr) {
    PacketKeys keys;
    const auto status = acquirePacketKeys(id.data(), id.size(), &keys);
    if (status != KeyResult::Ok) {
        CHECK(std::all_of(keys.odd.begin(), keys.odd.end(), [](uint8_t b) { return b == 0; }));
        CHECK(std::all_of(keys.even.begin(), keys.even.end(), [](uint8_t b) { return b == 0; }));
        return false;
    }
    if (result) {
        std::copy(keys.odd.begin(), keys.odd.end(), result->begin());
        std::copy(keys.even.begin(), keys.even.end(), result->begin() + 8);
    }
    return true;
}

void factoryAndAbi() {
    Environment env;
    std::vector<CasPluginDescriptor> descriptors;
    CHECK(env.factory->queryPlugins(&descriptors) == OK);
    CHECK(descriptors.size() == 1 && descriptors[0].CA_system_id == 5);
    CHECK(env.factory->isSystemIdSupported(5));
    CHECK(!env.factory->isSystemIdSupported(1));
    CHECK(!env.factory->isSystemIdSupported(0xf6d8));
    CHECK(env.factory->queryPlugins(nullptr) == BAD_VALUE);
    CasPlugin* unsupported = reinterpret_cast<CasPlugin*>(1);
    CHECK(env.factory->createPlugin(1, nullptr, static_cast<CasPluginCallbackExt>(nullptr), &unsupported) == ERROR_CAS_CANNOT_HANDLE);
    CHECK(unsupported == nullptr);
    CHECK(env.factory->createPlugin(5, nullptr, static_cast<CasPluginCallback>(nullptr), nullptr) == BAD_VALUE);
    for (bool legacy : {false, true}) {
        auto plugin = env.plugin(nullptr, legacy);
        CHECK(plugin->openSession(nullptr) == BAD_VALUE);
        CasSessionId rejected{9};
        CHECK(plugin->openSession(1, 8, &rejected) == ERROR_CAS_CANNOT_HANDLE && rejected.empty());
        CHECK(plugin->openSession(0, 0, &rejected) == ERROR_CAS_CANNOT_HANDLE && rejected.empty());
        auto id = open(*plugin);
        CasSessionId typed;
        CHECK(plugin->openSession(0, 8, &typed) == OK && typed != id);
        CHECK(plugin->setPrivateData(Bytes(251, 0x41)) == OK);
        CHECK(plugin->setPrivateData(Bytes(252, 0x41)) == BAD_VALUE);
        CHECK(plugin->setSessionPrivateData(id, {1, 2, 3}) == OK);
        CHECK(plugin->setSessionPrivateData(id, Bytes(252)) == BAD_VALUE);
        CHECK(plugin->sendEvent(1, 2, {}) == ERROR_CAS_CANNOT_HANDLE);
        CHECK(plugin->sendSessionEvent(id, 1, 2, {}) == ERROR_CAS_CANNOT_HANDLE);
        CHECK(plugin->provision(String8("unsupported")) == ERROR_CAS_CANNOT_HANDLE);
        CHECK(plugin->refreshEntitlements(0, {}) == ERROR_CAS_CANNOT_HANDLE);
        CHECK(plugin->closeSession(id) == OK);
        CHECK(plugin->closeSession(id) == ERROR_CAS_SESSION_NOT_OPENED);
        CHECK(plugin->processEcm(id, ecm()) == ERROR_CAS_SESSION_NOT_OPENED);
        CHECK(plugin->setSessionPrivateData(id, {}) == ERROR_CAS_SESSION_NOT_OPENED);
        CHECK(plugin->sendSessionEvent(id, 0, 0, {}) == ERROR_CAS_SESSION_NOT_OPENED);
        CHECK(plugin->closeSession(typed) == OK);
    }
}

struct CallbackState {
    CasPlugin* plugin = nullptr;
    std::vector<int32_t> capacities;
    CasSessionId reentrantSession;
    status_t reentrantStatus = UNKNOWN_ERROR;
};

void capacityCallback(void* data, int32_t event, int32_t count) {
    auto& state = *static_cast<CallbackState*>(data);
    CHECK(event == 1);
    state.capacities.push_back(count);
    if (state.plugin && state.reentrantSession.empty()) {
        state.reentrantStatus = state.plugin->openSession(&state.reentrantSession);
    }
}

void capacityAndReentrancy() {
    Environment env;
    CallbackState first, second;
    auto a = env.plugin(&first);
    auto b = env.plugin(&second);
    CHECK(first.capacities.empty());
    first.plugin = a.get();
    CHECK(a->setStatusCallback(capacityCallback) == OK);
    CHECK(first.reentrantStatus == OK);
    CHECK(b->setStatusCallback(capacityCallback) == OK);
    CHECK(first.capacities == std::vector<int32_t>{kSessionCapacity});
    CHECK(second.capacities == first.capacities);
    std::vector<CasSessionId> sessions;
    for (int i = 1; i < kSessionCapacity; ++i) sessions.push_back(open(*b));
    CasSessionId rejected{1, 2};
    CHECK(a->openSession(&rejected) == ERROR_CAS_RESOURCE_BUSY && rejected.empty());
    CHECK(a->closeSession(first.reentrantSession) == OK);
    CHECK(a->openSession(&rejected) == OK);
    CHECK(first.capacities.size() == 1 && second.capacities.size() == 1);
    a.reset();
    CHECK(first.capacities.size() == 1);
    CHECK(b->setStatusCallback(nullptr) == OK);
}

void sectionValidation() {
    Environment env;
    auto plugin = env.plugin();
    const auto id = open(*plugin);
    auto valid = ecm();
    for (size_t size = 0; size < valid.size(); ++size) {
        CHECK(plugin->processEcm(id, Bytes(valid.begin(), valid.begin() + size)) == BAD_VALUE);
    }
    auto extra = valid;
    extra.push_back(0);
    CHECK(plugin->processEcm(id, extra) == BAD_VALUE);
    auto badCrc = valid;
    badCrc.back() ^= 1;
    CHECK(plugin->processEcm(id, badCrc) == BAD_VALUE);
    CHECK(plugin->processEcm(id, section(0x84, Bytes(30))) == ERROR_CAS_CANNOT_HANDLE);
    CHECK(plugin->processEcm(id, section(0x82, Bytes(29))) == BAD_VALUE);
    CHECK(plugin->processEcm(id, section(0x82, Bytes(257))) == BAD_VALUE);
    auto messages = emmPayload(1, updateKey(2, kNextWorkKey));
    auto cut = messages;
    cut.pop_back();
    CHECK(plugin->processEmm(section(0x84, cut)) == BAD_VALUE);
    auto multiple = messages;
    multiple.insert(multiple.end(), cut.begin(), cut.end());
    CHECK(plugin->processEmm(section(0x84, multiple)) == BAD_VALUE);
    CHECK(plugin->processEcm(id, ecm(2, kNextWorkKey)) == ERROR_CAS_NO_LICENSE);
    auto oversized = Bytes(257);
    oversized[6] = 250;
    CHECK(plugin->processEmm(section(0x84, oversized)) == BAD_VALUE);
}

void ecmAndLifetime() {
    Environment env;
    auto a = env.plugin();
    auto b = env.plugin();
    const auto id = open(*a);
    const auto other = open(*b);
    CHECK(!resolve(id));
    CHECK(a->processEcm(id, ecm()) == OK);
    CHECK(b->processEcm(other, ecm()) == OK);
    std::array<uint8_t, 16> resolved{};
    CHECK(resolve(id, &resolved) && resolved == kKeys);
    CHECK(b->processEcm(id, ecm()) == ERROR_CAS_SESSION_NOT_OPENED);
    auto changed = kKeys;
    changed.fill(0x77);
    CHECK(a->processEcm(id, ecm(1, kWorkKey, changed)) == OK);
    CHECK(resolve(id, &resolved) && resolved == changed);
    CHECK(a->processEcm(id, ecm(1, kNextWorkKey)) == ERROR_CAS_DECRYPT);
    CHECK(resolve(id, &resolved) && resolved == changed);
    PacketKeys acquired;
    CHECK(acquirePacketKeys(id.data(), id.size(), &acquired) == KeyResult::Ok);
    CHECK(a->closeSession(id) == OK);
    CHECK(!resolve(id));
    CHECK(acquired.odd[0] == 0x77);
    CHECK(resolve(other));
    const auto replacement = open(*a);
    CHECK(replacement != id && !resolve(id));
    a.reset();
    CHECK(resolve(other));
    b.reset();
    CHECK(!resolve(other));
}

void emmUpdatesAndReplay() {
    Environment env;
    auto a = env.plugin();
    auto b = env.plugin();
    const auto id = open(*a);
    CHECK(a->processEcm(id, ecm(2, kNextWorkKey)) == ERROR_CAS_NO_LICENSE);
    const auto message = section(0x84, emmPayload(10, updateKey(2, kNextWorkKey)));
    CHECK(b->processEmm(message) == OK);
    CHECK(a->processEcm(id, ecm(2, kNextWorkKey)) == OK);
    std::unique_ptr<KeyReference> reference;
    CHECK(KeyReference::bind(id.data(), id.size(), &reference) == KeyResult::Ok);
    PacketKeys current;
    CHECK(reference->snapshot(&current) == KeyResult::Ok);
    CHECK(b->processEmm(message) == OK);
    CHECK(reference->snapshot(&current) == KeyResult::Ok);
    CHECK(b->processEmm(section(0x84, emmPayload(9, updateKey(3, kWorkKey)))) == ERROR_CAS_TAMPER_DETECTED);
    CHECK(b->processEmm(section(0x84, emmPayload(10, updateKey(2, kWorkKey)))) == ERROR_CAS_TAMPER_DETECTED);
    CHECK(b->processEmm(section(0x84, emmPayload(11, updateKey(2, kWorkKey)))) == ERROR_CAS_TAMPER_DETECTED);
    CHECK(a->processEcm(id, ecm(2, kNextWorkKey)) == OK);
    CHECK(b->processEmm(section(0x84, emmPayload(11, updateKey(12, kWorkKey)))) == OK);
    CHECK(reference->snapshot(&current) == KeyResult::UnknownToken);
    CHECK(a->processEcm(id, ecm(2, kNextWorkKey)) == ERROR_CAS_NO_LICENSE);
    CHECK(a->processEcm(id, ecm(12, kWorkKey)) == OK);
    CHECK(reference->snapshot(&current) == KeyResult::Ok);
    CHECK(b->processEmm(section(0x84, emmPayload(12, updateKey(2, kNextWorkKey)))) == ERROR_CAS_TAMPER_DETECTED);
}

void emmAtomicityAndRights() {
    Environment env;
    auto plugin = env.plugin();
    const auto id = open(*plugin);
    auto unknown = updateKey(2, kNextWorkKey);
    unknown.insert(unknown.end(), {0xee, 0});
    CHECK(plugin->processEmm(section(0x84, emmPayload(1, unknown))) == ERROR_CAS_CANNOT_HANDLE);
    CHECK(plugin->processEcm(id, ecm(2, kNextWorkKey)) == ERROR_CAS_NO_LICENSE);
    auto first = emmPayload(1, updateKey(2, kNextWorkKey));
    auto second = emmPayload(2, {0xee, 0});
    first.insert(first.end(), second.begin(), second.end());
    CHECK(plugin->processEmm(section(0x84, first)) == ERROR_CAS_CANNOT_HANDLE);
    CHECK(plugin->processEcm(id, ecm(2, kNextWorkKey)) == OK);
    CHECK(plugin->processEmm(section(0x84, first)) == ERROR_CAS_CANNOT_HANDLE);
    auto foreign = kCard;
    foreign[0] ^= 1;
    CHECK(plugin->processEmm(section(0x84, emmPayload(2, updateKey(3, kWorkKey), 0xffff, foreign))) == OK);
    CHECK(plugin->processEcm(id, ecm(3)) == ERROR_CAS_NO_LICENSE);
    CHECK(plugin->processEmm(section(0x84, emmPayload(2, updateKey(3, kWorkKey), 1))) == ERROR_CAS_LICENSE_EXPIRED);
    CHECK(plugin->processEcm(id, ecm(3)) == ERROR_CAS_NO_LICENSE);
    CHECK(plugin->processEcm(id, ecm(2, kNextWorkKey, kKeys, {0x52, 1, 2})) == ERROR_CAS_NO_LICENSE);
    CHECK(plugin->processEmm(section(0x84, emmPayload(2, {0x11, 1, 2}))) == OK);
    CHECK(plugin->processEcm(id, ecm(2, kNextWorkKey, kKeys, {0x52, 1, 2})) == OK);
    CHECK(plugin->processEcm(id, ecm(2, kNextWorkKey, kKeys, {0x52, 1, 4})) == ERROR_CAS_NO_LICENSE);
    CHECK(plugin->processEcm(id, ecm(2, kNextWorkKey, kKeys, {0x51, 1, 7})) == ERROR_CAS_CANNOT_HANDLE);
    CHECK(plugin->processEcm(id, ecm(2, kNextWorkKey, kKeys, {}, 4)) == ERROR_CAS_CANNOT_HANDLE);
    auto conflict = updateKey(3, kWorkKey);
    const auto next = updateKey(13, kNextWorkKey);
    conflict.insert(conflict.end(), next.begin(), next.end());
    CHECK(plugin->processEmm(section(0x84, emmPayload(3, conflict))) == BAD_VALUE);
    CHECK(plugin->processEcm(id, ecm(3)) == ERROR_CAS_NO_LICENSE);
}

void individualMessages() {
    Environment env;
    auto plugin = env.plugin();
    auto individual = individualPayload();
    CHECK(plugin->processEmm(section(0x85, individual, 1)) == ERROR_CAS_CANNOT_HANDLE);
    CHECK(plugin->processEmm(section(0x85, individual)) == ERROR_CAS_CANNOT_HANDLE);
    individual.back() ^= 1;
    CHECK(plugin->processEmm(section(0x85, individual)) == ERROR_CAS_DECRYPT);
    individual.pop_back();
    CHECK(plugin->processEmm(section(0x85, individual)) == BAD_VALUE);
}

void sharedReferenceFixedInput() {
    Environment env;
    auto plugin = env.plugin();
    const auto id = open(*plugin);
    CHECK(chmod(env.path.c_str(), 0666) == 0);
    std::unique_ptr<KeyReference> reference;
    CHECK(KeyReference::bind(id.data(), id.size(), &reference) == KeyResult::Unavailable);
    CHECK(plugin->processEcm(id, ecm()) == OK);
    CHECK(KeyReference::bind(id.data(), id.size(), &reference) == KeyResult::Ok);
    CHECK(chmod(env.path.c_str(), 0000) == 0);
    CHECK(unlink(env.path.c_str()) == 0);
    CHECK(resolve(id));
    PacketKeys current;
    // 受付処理の待機期限を越えても、初期入力の削除で共有状態を失効させない。
    std::this_thread::sleep_for(std::chrono::milliseconds(250));
    CHECK(reference->snapshot(&current) == KeyResult::Ok);
    std::unique_ptr<KeyReference> next;
    CHECK(KeyReference::bind(id.data(), id.size(), &next) == KeyResult::Ok);
    CHECK(next->snapshot(&current) == KeyResult::Ok);
    CHECK(plugin->processEmm(section(0x84, emmPayload(1, updateKey(2, kNextWorkKey)))) == OK);
    CHECK(!resolve(id));
    CHECK(plugin->processEcm(id, ecm(2, kNextWorkKey)) == OK);
    CHECK(resolve(id));
    CHECK(plugin->closeSession(id) == OK);
    CHECK(!resolve(id));
    CHECK(reference->snapshot(&current) == KeyResult::UnknownToken);
    CHECK(next->snapshot(&current) == KeyResult::UnknownToken);
}

void credentialsWithoutWorkKey() {
    Environment env;
    const int fd = ::open(env.path.c_str(), O_WRONLY | O_TRUNC);
    CHECK(fd >= 0);
    const char input[] = "# コメントだけでは作業鍵を得られない\n";
    CHECK(write(fd, input, sizeof(input) - 1) == sizeof(input) - 1);
    CHECK(close(fd) == 0);
    auto plugin = env.plugin();
    const auto id = open(*plugin);
    CHECK(plugin->processEcm(id, ecm()) == ERROR_CAS_NO_LICENSE);
    CHECK(!resolve(id));
}

void concurrentUpdatesAndClose() {
    Environment env;
    auto plugin = env.plugin();
    const auto id = open(*plugin);
    CHECK(plugin->processEcm(id, ecm()) == OK);
    auto next = kKeys;
    next.fill(0x55);
    std::atomic<bool> running{true};
    std::atomic<int> failures{0};
    std::thread reader([&] {
        while (running) {
            PacketKeys keys;
            if (acquirePacketKeys(id.data(), id.size(), &keys) == KeyResult::Ok) {
                const bool old = std::equal(keys.odd.begin(), keys.odd.end(), kKeys.begin()) &&
                                 std::equal(keys.even.begin(), keys.even.end(), kKeys.begin() + 8);
                const bool fresh = std::all_of(keys.odd.begin(), keys.odd.end(), [](uint8_t b) { return b == 0x55; }) &&
                                   std::all_of(keys.even.begin(), keys.even.end(), [](uint8_t b) { return b == 0x55; });
                if (!old && !fresh) ++failures;
            }
        }
    });
    for (int i = 0; i < 200; ++i) {
        if (plugin->processEcm(id, ecm(1, kWorkKey, i % 2 ? next : kKeys)) != OK) ++failures;
    }
    const auto result = plugin->closeSession(id);
    running = false;
    reader.join();
    CHECK(result == OK && failures == 0);
    CHECK(!resolve(id));
    CHECK(plugin->processEcm(id, ecm()) == ERROR_CAS_SESSION_NOT_OPENED);
}

void serviceDeathAndConsumerRestart() {
    Environment env;
    int ready[2], control[2];
    CHECK(pipe(ready) == 0 && pipe(control) == 0);
    const auto child = fork();
    CHECK(child >= 0);
    if (child == 0) {
        close(ready[0]); close(control[1]);
        auto plugin = env.plugin();
        const auto id = open(*plugin);
        if (plugin->processEcm(id, ecm()) != OK || write(ready[1], id.data(), id.size()) != 16) _exit(2);
        char byte;
        if (read(control[0], &byte, 1) < 0) _exit(4);
        _exit(3);
    }
    close(ready[1]); close(control[0]);
    CasSessionId id(16);
    CHECK(read(ready[0], id.data(), id.size()) == 16);
    std::unique_ptr<KeyReference> reference;
    CHECK(KeyReference::bind(id.data(), id.size(), &reference) == KeyResult::Ok);
    PacketKeys current;
    CHECK(reference->snapshot(&current) == KeyResult::Ok);
    // 独立 consumer の終了は CAS 側 session を失効させない。
    const auto consumer = fork();
    CHECK(consumer >= 0);
    if (consumer == 0) _exit(resolve(id) ? 0 : 1);
    int state;
    CHECK(waitpid(consumer, &state, 0) == consumer && WIFEXITED(state) && WEXITSTATUS(state) == 0);
    CHECK(resolve(id));
    CHECK(kill(child, SIGKILL) == 0);
    CHECK(waitpid(child, &state, 0) == child && WIFSIGNALED(state));
    CHECK(!resolve(id));
    CHECK(reference->snapshot(&current) == KeyResult::UnknownToken);
    auto restarted = env.plugin();
    const auto replacement = open(*restarted);
    CHECK(replacement != id);
    CHECK(restarted->processEcm(replacement, ecm()) == OK);
    CHECK(resolve(replacement) && !resolve(id));
    CHECK(reference->snapshot(&current) == KeyResult::UnknownToken);
    close(ready[0]); close(control[1]);
}

void boundedSocketAndInvalidRequests() {
    Environment env;
    auto plugin = env.plugin();
    const auto id = open(*plugin);
    CHECK(plugin->processEcm(id, ecm()) == OK);
    const int fd = socket(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC, 0);
    CHECK(fd >= 0);
    sockaddr_un address;
    const auto length = keySocketAddress(&address);
    CHECK(connect(fd, reinterpret_cast<sockaddr*>(&address), length) == 0);
    Bytes mutation(32, 0xa5);
    CHECK(send(fd, mutation.data(), mutation.size(), MSG_NOSIGNAL) == 32);
    pollfd item{fd, POLLIN, 0};
    CHECK(poll(&item, 1, 2000) > 0);
    uint8_t response[17];
    CHECK(recv(fd, response, sizeof(response), 0) <= 0);
    close(fd);
    CHECK(resolve(id));
    PacketKeys keys;
    CHECK(acquirePacketKeys(nullptr, 16, &keys) == KeyResult::InvalidToken);
    CHECK(acquirePacketKeys(id.data(), 1, &keys) == KeyResult::InvalidToken);
    auto wrong = id;
    wrong[0] ^= 1;
    CHECK(acquirePacketKeys(wrong.data(), wrong.size(), &keys) == KeyResult::UnknownToken);
    std::array<uint8_t, 8> odd{};
    std::array<uint8_t, 8> even{};
    even.fill(0xff);
    void* reference = nullptr;
    CHECK(maleicacid_cas_bind_key_reference(id.data(), id.size(), &reference) == MALEICACID_CAS_KEY_OK);
    CHECK(maleicacid_cas_snapshot_key_reference(reference, nullptr, even.data()) ==
          MALEICACID_CAS_KEY_INVALID_TOKEN);
    CHECK(std::all_of(even.begin(), even.end(), [](uint8_t b) { return b == 0; }));
    CHECK(maleicacid_cas_snapshot_key_reference(reference, odd.data(), even.data()) == MALEICACID_CAS_KEY_OK);
    CHECK(std::equal(odd.begin(), odd.end(), kKeys.begin()));
    CHECK(std::equal(even.begin(), even.end(), kKeys.begin() + 8));
    maleicacid_cas_release_key_reference(reference);
    reference = nullptr;
    CHECK(maleicacid_cas_bind_key_reference(wrong.data(), wrong.size(), &reference) ==
          MALEICACID_CAS_KEY_UNKNOWN_TOKEN);
    CHECK(reference == nullptr);
    CHECK(maleicacid_cas_snapshot_key_reference(reference, odd.data(), even.data()) ==
          MALEICACID_CAS_KEY_INVALID_TOKEN);
    CHECK(std::all_of(odd.begin(), odd.end(), [](uint8_t b) { return b == 0; }));
    CHECK(std::all_of(even.begin(), even.end(), [](uint8_t b) { return b == 0; }));
}

void keyResolutionStatus() {
    Environment env;
    auto plugin = env.plugin();
    const auto id = open(*plugin);
    CHECK(plugin->processEcm(id, ecm()) == OK);
    PacketKeys keys;
    const auto pending = open(*plugin);
    CHECK(acquirePacketKeys(pending.data(), pending.size(), &keys) == KeyResult::UnknownToken);
    CHECK(plugin->closeSession(pending) == OK);

    // 期限を過ぎた鍵を登録し、実際の照会経路で初回・再照会を確認する。
    auto& registry = KeyRegistry::instance();
    std::shared_ptr<KeyRegistry::Slot> slot;
    CHECK(registry.open(&slot) == Result::Ok);
    Secret<16> material;
    material.bytes = kKeys;
    CHECK(todayMjd() > 0);
    CHECK(registry.update(slot, material, 2, todayMjd() - 1) == Result::Ok);
    for (int attempt = 0; attempt < 2; ++attempt) {
        keys.odd.fill(0xff);
        keys.even.fill(0xff);
        CHECK(acquirePacketKeys(slot->token.data(), slot->token.size(), &keys) == KeyResult::UnknownToken);
        CHECK(std::all_of(keys.odd.begin(), keys.odd.end(), [](uint8_t b) { return b == 0; }));
        CHECK(std::all_of(keys.even.begin(), keys.even.end(), [](uint8_t b) { return b == 0; }));
    }
    CHECK(registry.update(slot, material, 2, todayMjd()) == Result::Ok);
    CHECK(acquirePacketKeys(slot->token.data(), slot->token.size(), &keys) == KeyResult::Ok);
    registry.close(slot);
    CHECK(acquirePacketKeys(slot->token.data(), slot->token.size(), &keys) == KeyResult::UnknownToken);

    CHECK(unlink(env.path.c_str()) == 0);
    for (int attempt = 0; attempt < 2; ++attempt) {
        CHECK(acquirePacketKeys(id.data(), id.size(), &keys) == KeyResult::UnknownToken);
    }
}

// これらは同一 process の backend/registry 試験であり、CasPlugin ABI や IPC の代替ではない。
std::shared_ptr<KeyRegistry::Slot> coreSlot() {
    std::shared_ptr<KeyRegistry::Slot> slot;
    CHECK(KeyRegistry::instance().open(&slot) == Result::Ok);
    return slot;
}

Result coreEcm(const std::shared_ptr<KeyRegistry::Slot>& slot, const Bytes& data) {
    View payload;
    const auto result = ecmPayload(data, &payload);
    return result == Result::Ok ? YakisobaBackend::instance().processEcm(slot, payload) : result;
}

Result coreEmm(const Bytes& data) {
    std::vector<EmmMessage> messages;
    const auto result = emmMessages(data, &messages);
    return result == Result::Ok ? YakisobaBackend::instance().processEmm(messages) : result;
}

Result coreResolve(const std::shared_ptr<KeyRegistry::Slot>& slot, std::array<uint8_t, 16>* output = nullptr) {
    Secret<16> keys;
    const auto result = YakisobaBackend::instance().resolve(slot->token, &keys);
    if (output) *output = keys.bytes;
    return result;
}

void coreFactoryDispatch() {
    Environment env;
    std::vector<CasPluginDescriptor> descriptors;
    CHECK(env.factory->queryPlugins(&descriptors) == OK);
    CHECK(descriptors.size() == 1 && descriptors[0].CA_system_id == 5);
    CHECK(env.factory->isSystemIdSupported(5));
    CHECK(!env.factory->isSystemIdSupported(1));
    CHECK(!env.factory->isSystemIdSupported(0xf6d8));
    CasPlugin* plugin = reinterpret_cast<CasPlugin*>(1);
    CHECK(env.factory->createPlugin(1, nullptr, static_cast<CasPluginCallback>(nullptr), &plugin) == ERROR_CAS_CANNOT_HANDLE);
    CHECK(plugin == nullptr);
    CHECK(env.factory->createPlugin(1, nullptr, static_cast<CasPluginCallbackExt>(nullptr), &plugin) == ERROR_CAS_CANNOT_HANDLE);
    CHECK(plugin == nullptr);
}

void coreParsing() {
    const auto data = ecm();
    View view;
    for (size_t i = 0; i < data.size(); ++i) {
        CHECK(ecmPayload(Bytes(data.begin(), data.begin() + i), &view) == Result::BadValue);
    }
    CHECK(ecmPayload(data, &view) == Result::Ok && view.size == 30);
    auto extra = data;
    extra.push_back(0);
    CHECK(ecmPayload(extra, &view) == Result::BadValue);
    extra = data;
    extra.back() ^= 1;
    CHECK(ecmPayload(extra, &view) == Result::BadValue);
    CHECK(ecmPayload(section(0x82, Bytes(257)), &view) == Result::BadValue);
    CHECK(ecmPayload(section(0x82, Bytes(29)), &view) == Result::BadValue);
    CHECK(ecmPayload(section(0x84, Bytes(30)), &view) == Result::Unsupported);
    auto payload = emmPayload(1, updateKey(2, kNextWorkKey));
    std::vector<EmmMessage> messages;
    auto whole = section(0x84, payload);
    CHECK(emmMessages(whole, &messages) == Result::Ok);
    CHECK(messages.size() == 1 && messages[0].payload.size == payload.size() && !messages[0].individual);
    auto cut = payload;
    cut.pop_back();
    payload.insert(payload.end(), cut.begin(), cut.end());
    CHECK(emmMessages(section(0x84, payload), &messages) == Result::BadValue && messages.empty());
    whole = section(0x85, individualPayload());
    CHECK(emmMessages(whole, &messages) == Result::Ok && messages[0].individual);
    CHECK(emmMessages(section(0x85, individualPayload(), 1), &messages) == Result::Unsupported);
    // CRC の既知値は MPEG-2 CRC の標準確認列を使う。
    const uint8_t sequence[] = {'1','2','3','4','5','6','7','8','9'};
    CHECK(sectionCrc({sequence, sizeof(sequence)}) == 0x0376e6e7);
}

void coreCapacityAndLifetime() {
    auto& registry = KeyRegistry::instance();
    std::vector<std::shared_ptr<KeyRegistry::Slot>> slots;
    std::set<Token> ids;
    for (int i = 0; i < kSessionCapacity; ++i) {
        auto slot = coreSlot();
        CHECK(ids.insert(slot->token).second);
        slots.push_back(std::move(slot));
    }
    std::shared_ptr<KeyRegistry::Slot> rejected;
    CHECK(registry.open(&rejected) == Result::Busy && !rejected);
    Secret<16> keys;
    keys.bytes = kKeys;
    auto first = slots.front();
    CHECK(registry.resolve(first->token, &keys) == Result::NoLicense);
    keys.bytes = kKeys;
    CHECK(registry.update(first, keys, 2, 0xffff) == Result::Ok);
    CHECK(registry.resolve(first->token, &keys) == Result::Ok && keys.bytes == kKeys);
    registry.close(first);
    CHECK(registry.resolve(first->token, &keys) == Result::SessionClosed);
    CHECK(registry.update(first, keys, 2, 0xffff) == Result::SessionClosed);
    const auto replacement = coreSlot();
    CHECK(!ids.count(replacement->token));
    registry.revokeAll();
    CHECK(registry.resolve(replacement->token, &keys) == Result::SessionClosed);
}

void coreEcmCommit() {
    Environment env;
    const auto slot = coreSlot();
    CHECK(coreResolve(slot) == Result::NotProvisioned);
    CHECK(coreEcm(slot, ecm()) == Result::Ok);
    std::array<uint8_t, 16> keys;
    CHECK(coreResolve(slot, &keys) == Result::Ok && keys == kKeys);
    auto next = kKeys;
    next.fill(0x77);
    CHECK(coreEcm(slot, ecm(1, kWorkKey, next)) == Result::Ok);
    CHECK(coreResolve(slot, &keys) == Result::Ok && keys == next);
    CHECK(coreEcm(slot, ecm(1, kNextWorkKey)) == Result::Decrypt);
    CHECK(coreResolve(slot, &keys) == Result::Ok && keys == next);
    CHECK(coreEcm(slot, ecm(2)) == Result::NoLicense);
    CHECK(coreEcm(slot, ecm(1, kWorkKey, next, {}, 4)) == Result::Unsupported);
    CHECK(coreResolve(slot, &keys) == Result::Ok && keys == next);
    KeyRegistry::instance().close(slot);
    CHECK(coreEcm(slot, ecm()) == Result::SessionClosed);
    CHECK(coreResolve(slot) == Result::SessionClosed);
}

void coreEmmWorkKeys() {
    Environment env;
    const auto slot = coreSlot();
    CHECK(coreEcm(slot, ecm(2, kNextWorkKey)) == Result::NoLicense);
    const auto message = section(0x84, emmPayload(10, updateKey(2, kNextWorkKey)));
    CHECK(coreEmm(message) == Result::Ok);
    CHECK(coreEcm(slot, ecm(2, kNextWorkKey)) == Result::Ok);
    CHECK(coreEmm(message) == Result::Ok && coreResolve(slot) == Result::Ok);
    CHECK(coreEmm(section(0x84, emmPayload(9, updateKey(3, kWorkKey)))) == Result::InvalidState);
    CHECK(coreEmm(section(0x84, emmPayload(10, updateKey(2, kWorkKey)))) == Result::InvalidState);
    CHECK(coreEmm(section(0x84, emmPayload(11, updateKey(2, kWorkKey)))) == Result::InvalidState);
    CHECK(coreEcm(slot, ecm(2, kNextWorkKey)) == Result::Ok);
    CHECK(coreEmm(section(0x84, emmPayload(11, updateKey(12, kWorkKey)))) == Result::Ok);
    CHECK(coreResolve(slot) == Result::NoLicense);
    CHECK(coreEcm(slot, ecm(2, kNextWorkKey)) == Result::NoLicense);
    CHECK(coreEcm(slot, ecm(12, kWorkKey)) == Result::Ok);
    CHECK(coreEmm(section(0x84, emmPayload(12, updateKey(2, kNextWorkKey)))) == Result::InvalidState);
}

void coreEmmAtomicity() {
    Environment env;
    const auto slot = coreSlot();
    auto unsupported = updateKey(2, kNextWorkKey);
    unsupported.insert(unsupported.end(), {0xee, 0});
    CHECK(coreEmm(section(0x84, emmPayload(1, unsupported))) == Result::Unsupported);
    CHECK(coreEcm(slot, ecm(2, kNextWorkKey)) == Result::NoLicense);
    auto messages = emmPayload(1, updateKey(2, kNextWorkKey));
    const auto bad = emmPayload(2, {0xee, 0});
    messages.insert(messages.end(), bad.begin(), bad.end());
    CHECK(coreEmm(section(0x84, messages)) == Result::Unsupported);
    CHECK(coreEcm(slot, ecm(2, kNextWorkKey)) == Result::Ok);
    CHECK(coreEmm(section(0x84, messages)) == Result::Unsupported);
    auto conflict = updateKey(3, kWorkKey);
    auto second = updateKey(13, kNextWorkKey);
    conflict.insert(conflict.end(), second.begin(), second.end());
    CHECK(coreEmm(section(0x84, emmPayload(3, conflict))) == Result::BadValue);
    CHECK(coreEcm(slot, ecm(3)) == Result::NoLicense);
}

void coreRightsAndIndividual() {
    Environment env;
    const auto slot = coreSlot();
    auto foreign = kCard;
    foreign[0] ^= 1;
    CHECK(coreEmm(section(0x84, emmPayload(1, updateKey(2, kNextWorkKey), 0xffff, foreign))) == Result::Ok);
    CHECK(coreEcm(slot, ecm(2, kNextWorkKey)) == Result::NoLicense);
    CHECK(coreEmm(section(0x84, emmPayload(1, updateKey(2, kNextWorkKey), 1))) == Result::Expired);
    CHECK(coreEcm(slot, ecm(2, kNextWorkKey)) == Result::NoLicense);
    CHECK(coreEcm(slot, ecm(1, kWorkKey, kKeys, {0x52, 1, 2})) == Result::NoLicense);
    CHECK(coreEmm(section(0x84, emmPayload(1, {0x11, 1, 2}))) == Result::Ok);
    CHECK(coreEcm(slot, ecm(1, kWorkKey, kKeys, {0x52, 1, 2})) == Result::Ok);
    CHECK(coreEcm(slot, ecm(1, kWorkKey, kKeys, {0x52, 1, 4})) == Result::NoLicense);
    CHECK(coreEcm(slot, ecm(1, kWorkKey, kKeys, {0x51, 1, 1})) == Result::Unsupported);
    CHECK(coreEcm(slot, ecm(1, kWorkKey, kKeys, {0x52, 32, 0})) == Result::BadValue);
    auto individual = individualPayload();
    CHECK(coreEmm(section(0x85, individual)) == Result::Unsupported);
    individual.back() ^= 1;
    CHECK(coreEmm(section(0x85, individual)) == Result::Decrypt);
}

void coreFixedCredential() {
    Environment env;
    auto slot = coreSlot();
    CHECK(chmod(env.path.c_str(), 0666) == 0);
    CHECK(coreEcm(slot, ecm()) == Result::Ok);
    const int fd = ::open(env.path.c_str(), O_WRONLY | O_TRUNC);
    CHECK(fd >= 0 && close(fd) == 0);
    CHECK(coreEcm(slot, ecm()) == Result::Ok);
    CHECK(chmod(env.path.c_str(), 0000) == 0);
    CHECK(coreResolve(slot) == Result::Ok);
    CHECK(unlink(env.path.c_str()) == 0);
    CHECK(coreEmm(section(0x84, emmPayload(1, updateKey(2, kNextWorkKey)))) == Result::Ok);
    CHECK(coreResolve(slot) == Result::NoLicense);
    CHECK(coreEcm(slot, ecm(2, kNextWorkKey)) == Result::Ok);
    CHECK(coreResolve(slot) == Result::Ok);
    KeyRegistry::instance().close(slot);
    CHECK(coreResolve(slot) == Result::SessionClosed);
}

void coreCredentialInputBounds() {
    Environment env;
    auto slot = coreSlot();
    const std::string saved = env.path + ".saved";
    CHECK(rename(env.path.c_str(), saved.c_str()) == 0);
    CHECK(coreEcm(slot, ecm()) == Result::NotProvisioned);
    CHECK(symlink(saved.c_str(), env.path.c_str()) == 0);
    CHECK(coreEcm(slot, ecm()) == Result::NotProvisioned);
    CHECK(unlink(env.path.c_str()) == 0);
    CHECK(mkdir(env.path.c_str(), 0700) == 0);
    CHECK(coreEcm(slot, ecm()) == Result::NotProvisioned);
    CHECK(rmdir(env.path.c_str()) == 0);
    CHECK(mkfifo(env.path.c_str(), 0600) == 0);
    CHECK(coreEcm(slot, ecm()) == Result::NotProvisioned);
    CHECK(unlink(env.path.c_str()) == 0);
    const int fd = ::open(env.path.c_str(), O_WRONLY | O_CREAT | O_EXCL, 0600);
    CHECK(fd >= 0);
    CHECK(coreEcm(slot, ecm()) == Result::NotProvisioned);
    CHECK(ftruncate(fd, 16385) == 0);
    CHECK(coreEcm(slot, ecm()) == Result::NotProvisioned);
    CHECK(close(fd) == 0);
    CHECK(rename(saved.c_str(), env.path.c_str()) == 0);
    CHECK(coreEcm(slot, ecm()) == Result::Ok);
}

void coreCredentialGrammar() {
    Environment env;
    const int fd = ::open(env.path.c_str(), O_WRONLY | O_TRUNC);
    CHECK(fd >= 0);
    const char input[] =
        "cardid: 01 02 03 04 05 06 07 08\n"
        "cardkey: 10 11 12 13 14 15 16 17\n"
        "key [02] [01]: 30 31 32 33 34 35 36 37\n"
        "key [02] [0b]: 40 41 42 43 44 45 46 47\n";
    CHECK(write(fd, input, sizeof(input) - 1) == sizeof(input) - 1);
    CHECK(close(fd) == 0);
    auto slot = coreSlot();
    CHECK(coreEcm(slot, ecm(11, kNextWorkKey)) == Result::Ok);
    CHECK(coreEcm(slot, ecm()) == Result::NoLicense);
    CHECK(coreEmm(section(0x84, emmPayload(1, updateKey(2, kWorkKey)))) == Result::Ok);
    CHECK(coreEcm(slot, ecm(2)) == Result::Ok);
}

void coreConcurrency() {
    Environment env;
    auto slot = coreSlot();
    CHECK(coreEcm(slot, ecm()) == Result::Ok);
    std::atomic<int> errors{0};
    auto next = kKeys;
    next.fill(0x55);
    std::thread reader([&] {
        for (int i = 0; i < 500; ++i) {
            std::array<uint8_t, 16> keys;
            if (coreResolve(slot, &keys) != Result::Ok || (keys != kKeys && keys != next)) ++errors;
        }
    });
    for (int i = 0; i < 500; ++i) {
        if (coreEcm(slot, ecm(1, kWorkKey, i % 2 ? kKeys : next)) != Result::Ok) ++errors;
    }
    reader.join();
    CHECK(errors == 0);
    KeyRegistry::instance().close(slot);
    CHECK(coreEcm(slot, ecm()) == Result::SessionClosed);
}

void coreAccessPolicy() {
    CHECK(peerMatchesPolicy(1013, "u:r:hal_cas_default:s0", true));
    CHECK(peerMatchesPolicy(1000, "u:r:hal_tv_tuner_default:s0", false));
    CHECK(!peerMatchesPolicy(1013, "u:r:hal_tv_tuner_default:s0", false));
    CHECK(!peerMatchesPolicy(1000, "u:r:untrusted_app:s0", false));
    CHECK(!peerMatchesPolicy(0, "u:r:hal_cas_default:s0", true));
    CHECK(!peerMatchesPolicy(1013, nullptr, true));
    CHECK(!peerMatchesPolicy(1013, "u:r:hal_cas_default:s0:c1", true));
}

}  // namespace

void sharedReferenceUpdatesAndRevocation() {
    Environment env;
    auto plugin = env.plugin();
    const auto id = open(*plugin);
    CHECK(plugin->processEcm(id, ecm()) == OK);
    std::unique_ptr<KeyReference> reference;
    CHECK(KeyReference::bind(id.data(), id.size(), &reference) == KeyResult::Ok);
    PacketKeys first;
    const auto queries = KeyReference::bindingQueriesForTest();
    for (int packet = 0; packet < 1000; ++packet) {
        CHECK(reference->snapshot(&first) == KeyResult::Ok);
        CHECK(std::equal(first.odd.begin(), first.odd.end(), kKeys.begin()));
    }
    CHECK(KeyReference::bindingQueriesForTest() == queries);
    auto next = kKeys;
    next.fill(0x73);
    CHECK(plugin->processEcm(id, ecm(1, kWorkKey, next)) == OK);
    PacketKeys current;
    CHECK(reference->snapshot(&current) == KeyResult::Ok && current.odd[0] == 0x73);
    CHECK(plugin->processEcm(id, ecm(1, kNextWorkKey)) == ERROR_CAS_DECRYPT);
    CHECK(reference->snapshot(&current) == KeyResult::Ok && current.odd[0] == 0x73);
    CHECK(std::equal(first.odd.begin(), first.odd.end(), kKeys.begin()));
    // CAS が失効を確定する。TIS の処理や再結合は呼ばない。
    KeyRegistry::instance().invalidateGroup(2);
    CHECK(reference->snapshot(&current) == KeyResult::UnknownToken);
    CHECK(plugin->processEcm(id, ecm(1, kNextWorkKey)) == ERROR_CAS_DECRYPT);
    CHECK(reference->snapshot(&current) == KeyResult::UnknownToken);
    CHECK(plugin->processEcm(id, ecm()) == OK);
    CHECK(reference->snapshot(&current) == KeyResult::Ok);
    CHECK(plugin->closeSession(id) == OK);
    CHECK(reference->snapshot(&current) == KeyResult::UnknownToken);
    CHECK(plugin->processEcm(id, ecm()) == ERROR_CAS_SESSION_NOT_OPENED);
    CHECK(reference->snapshot(&current) == KeyResult::UnknownToken);
}

void sharedStateConsistencyAndReadOnlyMapping() {
    auto slot = SharedSlot::create();
    CHECK(slot != nullptr);
    const int reader = slot->readerFd();
    CHECK(reader >= 0);
    CHECK((fcntl(reader, F_GETFL) & O_ACCMODE) == O_RDONLY);
    char path[64];
    snprintf(path, sizeof(path), "/proc/self/fd/%d", reader);
    const int writable = ::open(path, O_RDWR | O_CLOEXEC);
    CHECK(writable >= 0);
    const uint8_t byte = 0;
    CHECK(write(writable, &byte, 1) == -1);
    CHECK(mmap(nullptr, sizeof(SharedKeyState), PROT_READ | PROT_WRITE, MAP_SHARED, writable, 0) == MAP_FAILED);
    close(writable);
    CHECK(mmap(nullptr, sizeof(SharedKeyState), PROT_READ | PROT_WRITE, MAP_SHARED, reader, 0) == MAP_FAILED);
    const auto* state = static_cast<const SharedKeyState*>(
        mmap(nullptr, sizeof(SharedKeyState), PROT_READ, MAP_SHARED, reader, 0));
    CHECK(state != MAP_FAILED);
    close(reader);
    Secret<16> first;
    first.bytes.fill(0x21);
    slot->state().store(&first, 0xffff);
    std::atomic<bool> finished{false};
    std::thread writer([&] {
        Secret<16> next;
        for (int i = 0; i < 10000; ++i) {
            next.bytes.fill(i % 2 ? 0x21 : 0x42);
            slot->state().store(&next, 0xffff);
        }
        finished = true;
    });
    bool consistent = true;
    size_t reads = 0;
    do {
        Secret<16> current;
        const auto result = state->snapshot(&current);
        if (result == Result::Ok) {
            ++reads;
            consistent &= current.bytes[0] == 0x21 || current.bytes[0] == 0x42;
            consistent &= std::all_of(current.bytes.begin(), current.bytes.end(),
                [&](uint8_t byte) { return byte == current.bytes[0]; });
        } else consistent &= result == Result::Busy;
    } while (!finished || reads < 1000);
    writer.join();
    CHECK(consistent);
    slot.reset();
    Secret<16> current;
    CHECK(state->snapshot(&current) == Result::NoLicense);
    CHECK(munmap(const_cast<SharedKeyState*>(state), sizeof(SharedKeyState)) == 0);
    SharedKeyState exhausted;
    exhausted.sequence.store(UINT64_MAX - 1);
    CHECK(!exhausted.store(&first, 0xffff));
    CHECK(exhausted.snapshot(&current) == Result::SessionClosed);
}

using TestCase = std::pair<const char*, std::function<void()>>;

std::vector<TestCase> testCases(bool coreOnly) {
    const std::vector<std::pair<const char*, std::function<void()>>> integration = {
        {"factory_abi", factoryAndAbi},
        {"capacity_reentrancy", capacityAndReentrancy},
        {"section_validation", sectionValidation},
        {"ecm_lifetime", ecmAndLifetime},
        {"emm_replay", emmUpdatesAndReplay},
        {"emm_atomicity_rights", emmAtomicityAndRights},
        {"individual_messages", individualMessages},
        {"shared_reference_fixed_input", sharedReferenceFixedInput},
        {"credentials_without_work_key", credentialsWithoutWorkKey},
        {"concurrent_update_close", concurrentUpdatesAndClose},
        {"service_death_consumer_restart", serviceDeathAndConsumerRestart},
        {"socket_input_bounds", boundedSocketAndInvalidRequests},
        {"key_resolution_status", keyResolutionStatus},
        {"shared_reference_updates_revoke", sharedReferenceUpdatesAndRevocation},
    };
    std::vector<std::pair<const char*, std::function<void()>>> tests = {
        {"core_factory_dispatch", coreFactoryDispatch},
        {"core_section_parser", coreParsing},
        {"core_capacity_lifetime", coreCapacityAndLifetime},
        {"core_ecm_commit", coreEcmCommit},
        {"core_emm_work_keys", coreEmmWorkKeys},
        {"core_emm_atomicity", coreEmmAtomicity},
        {"core_rights_individual", coreRightsAndIndividual},
        {"core_fixed_credential", coreFixedCredential},
        {"core_credential_input_bounds", coreCredentialInputBounds},
        {"core_credential_grammar", coreCredentialGrammar},
        {"core_concurrency", coreConcurrency},
        {"core_access_policy", coreAccessPolicy},
        {"core_shared_state", sharedStateConsistencyAndReadOnlyMapping},
    };
    if (!coreOnly) tests.insert(tests.end(), integration.begin(), integration.end());
    return tests;
}

int isolatedTest(const TestCase& testCase) {
    fflush(nullptr);
    const auto child = fork();
    if (child < 0) return 2;
    if (child == 0) {
        // 失敗した試験の worker や IPC が runner を永久占有しない。
        alarm(20);
        try { testCase.second(); _exit(0); }
        catch (const std::exception& error) {
            fprintf(stderr, "%s: %s\n", testCase.first, error.what());
            _exit(1);
        }
    }
    int state;
    if (waitpid(child, &state, 0) != child || !WIFEXITED(state)) return 2;
    return WEXITSTATUS(state);
}

#ifdef __ANDROID__
// atest/Tradefed が列挙・実行・集計できる GoogleTest の入口を使用する。
class B25CasTest : public testing::TestWithParam<TestCase> {};
TEST_P(B25CasTest, Contract) { EXPECT_EQ(isolatedTest(GetParam()), 0); }
INSTANTIATE_TEST_SUITE_P(B25, B25CasTest, testing::ValuesIn(testCases(false)),
    [](const testing::TestParamInfo<TestCase>& info) { return std::string(info.param.first); });
#else
int main(int argc, char** argv) {
    const bool coreOnly = argc == 2 && strcmp(argv[1], "--core") == 0;
    if (argc != 1 && !coreOnly) return 2;
    const auto tests = testCases(coreOnly);
    int failures = 0;
    for (const auto& test : tests) {
        if (isolatedTest(test) != 0) {
            ++failures;
            printf("FAIL %s\n", test.first);
        } else {
            printf("PASS %s\n", test.first);
        }
    }
    printf("%zu suites, %d failures\n", std::size(tests), failures);
    return failures == 0 ? 0 : 1;
}
#endif
