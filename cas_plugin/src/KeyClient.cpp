// SPDX-License-Identifier: GPL-3.0-only
#include <maleicacid/cas/KeyClient.h>

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
    if (received != 17 || response.bytes[0] != 0) return KeyResult::Unavailable;
    std::copy_n(response.bytes.data() + 1, 8, output->odd.data());
    std::copy_n(response.bytes.data() + 9, 8, output->even.data());
    return KeyResult::Ok;
}

}  // namespace maleicacid::cas
