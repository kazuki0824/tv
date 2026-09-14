// SPDX-License-Identifier: GPL-3.0-only
#include "KeyServer.h"

#include "KeySocket.h"
#include "YakisobaBackend.h"

#include <algorithm>
#include <cerrno>
#include <poll.h>
#include <unistd.h>

namespace maleicacid::cas {

std::shared_ptr<KeyServer> KeyServer::acquire() {
    static std::mutex mutex;
    static std::weak_ptr<KeyServer> current;
    std::lock_guard lock(mutex);
    auto server = current.lock();
    if (!server) {
        server = std::make_shared<KeyServer>();
        if (!server->start()) return {};
        current = server;
    }
    return server->healthy() ? server : nullptr;
}

bool KeyServer::start() {
    fd_ = socket(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC | SOCK_NONBLOCK, 0);
    if (fd_ < 0) return false;
    sockaddr_un address;
    const auto size = keySocketAddress(&address);
    if (bind(fd_, reinterpret_cast<sockaddr*>(&address), size) || listen(fd_, kSessionCapacity)) {
        return false;
    }
    healthy_ = true;
    thread_ = std::thread([this] { run(); });
    return true;
}

KeyServer::~KeyServer() {
    stop_ = true;
    healthy_ = false;
    if (fd_ >= 0) shutdown(fd_, SHUT_RDWR);
    if (thread_.joinable()) thread_.join();
    if (fd_ >= 0) close(fd_);
}

void KeyServer::serve(int fd) {
    struct Fd { int value; ~Fd() { close(value); } } cleanup{fd};
    if (!authorizedPeer(fd, false) || !waitSocket(fd, POLLIN)) return;
    Token token;
    if (recv(fd, token.data(), token.size(), MSG_TRUNC) != static_cast<ssize_t>(token.size())) return;
    Secret<16> keys;
    const auto result = YakisobaBackend::instance().resolve(token, &keys);
    Secret<17> response;
    const auto status = result == Result::Ok
                            ? KeyResponseStatus::Ok
                            : result == Result::SessionClosed ? KeyResponseStatus::UnknownToken
                                                              : KeyResponseStatus::Unavailable;
    response.bytes[0] = static_cast<uint8_t>(status);
    if (result == Result::Ok) std::copy(keys.bytes.begin(), keys.bytes.end(), response.bytes.begin() + 1);
    if (waitSocket(fd, POLLOUT)) send(fd, response.bytes.data(), response.bytes.size(), MSG_NOSIGNAL);
}

void KeyServer::run() {
    try {
        while (!stop_) {
            pollfd item{fd_, POLLIN, 0};
            const auto ready = poll(&item, 1, kSocketDeadlineMs);
            if (stop_) break;
            if (ready == 0 || (ready < 0 && errno == EINTR)) continue;
            if (ready < 0 || (item.revents & (POLLERR | POLLHUP | POLLNVAL))) break;
            const int client = accept4(fd_, nullptr, nullptr, SOCK_CLOEXEC | SOCK_NONBLOCK);
            if (client < 0) {
                if (errno == EAGAIN || errno == EINTR) continue;
                break;
            }
            serve(client);
        }
    } catch (...) {
        // worker の異常終了を、新しい鍵取得が成功する状態へ戻さない。
    }
    healthy_ = false;
    if (!stop_) KeyRegistry::instance().revokeAll();
}

}  // namespace maleicacid::cas
