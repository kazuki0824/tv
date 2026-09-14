// SPDX-License-Identifier: GPL-3.0-only
#include <maleicacid/cas/KeyClient.h>
#include <maleicacid/cas/KeyClientC.h>

#include "KeySocket.h"
#include "Types.h"

#include <algorithm>
#include <cerrno>
#include <poll.h>
#include <unistd.h>

namespace maleicacid::cas {

PacketKeys::~PacketKeys() {
    eraseSecret(odd.data(), odd.size());
    eraseSecret(even.data(), even.size());
}

KeyResult acquirePacketKeys(const uint8_t* token, size_t length, PacketKeys* output) {
    if (output == nullptr) return KeyResult::InvalidToken;
    eraseSecret(output->odd.data(), output->odd.size());
    eraseSecret(output->even.data(), output->even.size());
    if (token == nullptr || length != 16) return KeyResult::InvalidToken;
    const int fd = socket(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC | SOCK_NONBLOCK, 0);
    if (fd < 0) return KeyResult::Unavailable;
    struct Fd { int value; ~Fd() { close(value); } } cleanup{fd};
    sockaddr_un address;
    const auto size = keySocketAddress(&address);
    if (connect(fd, reinterpret_cast<sockaddr*>(&address), size) != 0) {
        return KeyResult::Unavailable;
    }
    if (!authorizedPeer(fd, true) || !waitSocket(fd, POLLOUT) ||
        send(fd, token, length, MSG_NOSIGNAL) != static_cast<ssize_t>(length) ||
        !waitSocket(fd, POLLIN)) return KeyResult::Unavailable;
    Secret<17> response;
    const auto received = recv(fd, response.bytes.data(), response.bytes.size(), MSG_TRUNC);
    if (received != 17) return KeyResult::Unavailable;
    const auto status = static_cast<KeyResponseStatus>(response.bytes[0]);
    if (status == KeyResponseStatus::UnknownToken) return KeyResult::UnknownToken;
    if (status != KeyResponseStatus::Ok) return KeyResult::Unavailable;
    std::copy_n(response.bytes.data() + 1, 8, output->odd.data());
    std::copy_n(response.bytes.data() + 9, 8, output->even.data());
    return KeyResult::Ok;
}

}  // namespace maleicacid::cas

extern "C" int maleicacid_cas_acquire_packet_keys(const uint8_t* token, size_t length,
                                                    uint8_t* odd, uint8_t* even) {
    if (odd != nullptr) std::fill_n(odd, 8, 0);
    if (even != nullptr) std::fill_n(even, 8, 0);
    if (odd == nullptr || even == nullptr) return MALEICACID_CAS_KEY_INVALID_TOKEN;
    maleicacid::cas::PacketKeys keys;
    switch (maleicacid::cas::acquirePacketKeys(token, length, &keys)) {
        case maleicacid::cas::KeyResult::Ok:
            std::copy(keys.odd.begin(), keys.odd.end(), odd);
            std::copy(keys.even.begin(), keys.even.end(), even);
            return MALEICACID_CAS_KEY_OK;
        case maleicacid::cas::KeyResult::InvalidToken:
            return MALEICACID_CAS_KEY_INVALID_TOKEN;
        case maleicacid::cas::KeyResult::UnknownToken:
            return MALEICACID_CAS_KEY_UNKNOWN_TOKEN;
        case maleicacid::cas::KeyResult::Unavailable:
            return MALEICACID_CAS_KEY_UNAVAILABLE;
    }
    return MALEICACID_CAS_KEY_UNAVAILABLE;
}
