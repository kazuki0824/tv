// SPDX-License-Identifier: GPL-3.0-only
#include <maleicacid/cas/KeyClient.h>
#include <maleicacid/cas/KeyClientC.h>

#include "KeySocket.h"
#include "SharedSlot.h"

#include <algorithm>
#include <cstring>
#include <fcntl.h>
#include <new>
#include <poll.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

namespace maleicacid::cas {
namespace {
#ifdef MALEICACID_CAS_TEST
std::atomic<uint64_t> bindingQueries{0};
#endif
struct Fd {
    int value = -1;
    ~Fd() { if (value >= 0) close(value); }
};

bool ownerAlive(int fd) {
    pollfd owner{fd, POLLIN, 0};
    // pidfd は所有 process の終了を kernel が通知する。相手への要求送信はない。
    // EINTR を含む検査不能時も、新しい packet に鍵を渡さない。
    return poll(&owner, 1, 0) == 0;
}

CasScheme toCasScheme(KeyScheme scheme) {
    switch (scheme) {
        case KeyScheme::B25: return CasScheme::B25;
        case KeyScheme::B1: return CasScheme::B1;
        case KeyScheme::Unknown: return CasScheme::Unknown;
    }
    return CasScheme::Unknown;
}
}

#ifdef MALEICACID_CAS_TEST
uint64_t KeyReference::bindingQueriesForTest() { return bindingQueries.load(); }
#endif

PacketKeys::~PacketKeys() {
    eraseSecret(odd.data(), odd.size());
    eraseSecret(even.data(), even.size());
}

KeyReference::~KeyReference() {
    munmap(const_cast<SharedKeyState*>(state_), sizeof(SharedKeyState));
    close(owner_);
}

KeyResult KeyReference::snapshot(PacketKeys* output) const {
    if (output == nullptr) return KeyResult::InvalidToken;
    eraseSecret(output->odd.data(), output->odd.size());
    eraseSecret(output->even.data(), output->even.size());
    if (!ownerAlive(owner_)) return KeyResult::UnknownToken;
    Secret<16> keys;
    const auto result = state_->snapshot(&keys);
    if (!ownerAlive(owner_)) return KeyResult::UnknownToken;
    if (result == Result::Busy) return KeyResult::Unavailable;
    if (result != Result::Ok) return KeyResult::UnknownToken;
    std::copy_n(keys.bytes.data(), 8, output->odd.data());
    std::copy_n(keys.bytes.data() + 8, 8, output->even.data());
    return KeyResult::Ok;
}

KeyResult KeyReference::bind(const uint8_t* token, size_t length,
                             std::unique_ptr<KeyReference>* output) {
    if (output == nullptr) return KeyResult::InvalidToken;
    output->reset();
    if (token == nullptr || length != 16) return KeyResult::InvalidToken;
#ifdef MALEICACID_CAS_TEST
    ++bindingQueries;
#endif
    Fd connection{socket(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC | SOCK_NONBLOCK, 0)};
    const int fd = connection.value;
    if (fd < 0) return KeyResult::Unavailable;
    sockaddr_un address;
    const auto size = keySocketAddress(&address);
    if (connect(fd, reinterpret_cast<sockaddr*>(&address), size) != 0) {
        return KeyResult::Unavailable;
    }
    if (!authorizedPeer(fd, true) || !waitSocket(fd, POLLOUT) ||
        send(fd, token, length, MSG_NOSIGNAL) != static_cast<ssize_t>(length) ||
        !waitSocket(fd, POLLIN)) return KeyResult::Unavailable;
    KeyBindResponse response;
    iovec data{&response, sizeof(response)};
    alignas(cmsghdr) char control[CMSG_SPACE(2 * sizeof(int))]{};
    msghdr message{};
    message.msg_iov = &data;
    message.msg_iovlen = 1;
    message.msg_control = control;
    message.msg_controllen = sizeof(control);
    const auto received = recvmsg(fd, &message, MSG_CMSG_CLOEXEC | MSG_TRUNC);
    Fd descriptors[2];
    size_t count = 0;
    bool malformed = false;
    for (auto* header = CMSG_FIRSTHDR(&message); header; header = CMSG_NXTHDR(&message, header)) {
        if (header->cmsg_level != SOL_SOCKET || header->cmsg_type != SCM_RIGHTS ||
            header->cmsg_len < CMSG_LEN(0)) {
            malformed = true;
            continue;
        }
        const size_t bytes = header->cmsg_len - CMSG_LEN(0);
        if (bytes % sizeof(int)) malformed = true;
        for (size_t offset = 0; offset + sizeof(int) <= bytes; offset += sizeof(int)) {
            int descriptor;
            std::memcpy(&descriptor, CMSG_DATA(header) + offset, sizeof(descriptor));
            if (count < 2) descriptors[count++].value = descriptor;
            else { close(descriptor); malformed = true; }
        }
    }
    if (received != static_cast<ssize_t>(sizeof(response)) || malformed ||
        (message.msg_flags & (MSG_TRUNC | MSG_CTRUNC))) {
        return KeyResult::Unavailable;
    }
    if (response.status == KeyResponseStatus::UnknownToken && count == 0) {
        return KeyResult::UnknownToken;
    }
    const auto scheme = toCasScheme(response.scheme);
    if (response.status != KeyResponseStatus::Ok || count != 2 || scheme == CasScheme::Unknown) {
        return KeyResult::Unavailable;
    }
    struct stat info{};
    const int memory = descriptors[0].value;
    const int seals = fcntl(memory, F_GET_SEALS);
    constexpr int required = F_SEAL_GROW | F_SEAL_SHRINK | F_SEAL_FUTURE_WRITE | F_SEAL_SEAL;
    if (fstat(memory, &info) || info.st_size != sizeof(SharedKeyState) ||
        (fcntl(memory, F_GETFL) & O_ACCMODE) != O_RDONLY ||
        seals < 0 || (seals & required) != required) return KeyResult::Unavailable;
    void* mapping = mmap(nullptr, sizeof(SharedKeyState), PROT_READ, MAP_SHARED, memory, 0);
    if (mapping == MAP_FAILED) return KeyResult::Unavailable;
    auto reference = std::unique_ptr<KeyReference>(new (std::nothrow)
        KeyReference(static_cast<const SharedKeyState*>(mapping), descriptors[1].value, scheme));
    if (!reference) {
        munmap(mapping, sizeof(SharedKeyState));
        return KeyResult::Unavailable;
    }
    descriptors[1].value = -1;
    PacketKeys current;
    const auto result = reference->snapshot(&current);
    if (result == KeyResult::Ok) *output = std::move(reference);
    return result;
}

}  // namespace maleicacid::cas

namespace {
int keyStatus(maleicacid::cas::KeyResult result) {
    switch (result) {
        case maleicacid::cas::KeyResult::Ok: return MALEICACID_CAS_KEY_OK;
        case maleicacid::cas::KeyResult::InvalidToken: return MALEICACID_CAS_KEY_INVALID_TOKEN;
        case maleicacid::cas::KeyResult::UnknownToken: return MALEICACID_CAS_KEY_UNKNOWN_TOKEN;
        case maleicacid::cas::KeyResult::Unavailable: return MALEICACID_CAS_KEY_UNAVAILABLE;
    }
    return MALEICACID_CAS_KEY_UNAVAILABLE;
}

uint8_t casScheme(maleicacid::cas::CasScheme scheme) {
    switch (scheme) {
        case maleicacid::cas::CasScheme::B25: return MALEICACID_CAS_SCHEME_B25;
        case maleicacid::cas::CasScheme::B1: return MALEICACID_CAS_SCHEME_B1;
        case maleicacid::cas::CasScheme::Unknown: return MALEICACID_CAS_SCHEME_UNKNOWN;
    }
    return MALEICACID_CAS_SCHEME_UNKNOWN;
}
}

extern "C" int maleicacid_cas_bind_key_reference(const uint8_t* token, size_t length,
                                                 void** reference) {
    if (reference == nullptr) return MALEICACID_CAS_KEY_INVALID_TOKEN;
    *reference = nullptr;
    std::unique_ptr<maleicacid::cas::KeyReference> result;
    const auto status = maleicacid::cas::KeyReference::bind(token, length, &result);
    *reference = result.release();
    return keyStatus(status);
}

extern "C" uint8_t maleicacid_cas_key_reference_scheme(const void* reference) {
    if (reference == nullptr) return MALEICACID_CAS_SCHEME_UNKNOWN;
    return casScheme(static_cast<const maleicacid::cas::KeyReference*>(reference)->scheme());
}

extern "C" void maleicacid_cas_release_key_reference(void* reference) {
    delete static_cast<maleicacid::cas::KeyReference*>(reference);
}

extern "C" int maleicacid_cas_snapshot_key_reference(const void* reference, uint8_t* odd,
                                                     uint8_t* even) {
    if (odd != nullptr) std::fill_n(odd, 8, 0);
    if (even != nullptr) std::fill_n(even, 8, 0);
    if (reference == nullptr || odd == nullptr || even == nullptr) {
        return MALEICACID_CAS_KEY_INVALID_TOKEN;
    }
    maleicacid::cas::PacketKeys keys;
    const auto status = static_cast<const maleicacid::cas::KeyReference*>(reference)->snapshot(&keys);
    if (status == maleicacid::cas::KeyResult::Ok) {
        std::copy(keys.odd.begin(), keys.odd.end(), odd);
        std::copy(keys.even.begin(), keys.even.end(), even);
    }
    return keyStatus(status);
}
