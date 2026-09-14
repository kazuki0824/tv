// SPDX-License-Identifier: GPL-3.0-only
#include "KeyServer.h"

#include "KeySocket.h"
#include "YakisobaBackend.h"

#include <cerrno>
#include <poll.h>
#include <cstring>
#include <sys/syscall.h>
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
    ownerFd_ = syscall(SYS_pidfd_open, getpid(), 0);
    if (ownerFd_ < 0) return false;
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
    if (ownerFd_ >= 0) close(ownerFd_);
}

void KeyServer::serve(int fd) {
    struct Fd { int value; ~Fd() { if (value >= 0) close(value); } } cleanup{fd};
    if (!authorizedPeer(fd, false) || !waitSocket(fd, POLLIN)) return;
    Token token;
    if (recv(fd, token.data(), token.size(), MSG_TRUNC) != static_cast<ssize_t>(token.size())) return;
    int slotFd = -1;
    const auto result = YakisobaBackend::instance().bind(token, &slotFd);
    Fd slotCleanup{slotFd};
    auto status = KeyResponseStatus::Unavailable;
    switch (result) {
        case Result::Ok:
            status = KeyResponseStatus::Ok;
            break;
        case Result::NoLicense:
        case Result::SessionClosed:
        case Result::Revoked:
            // 有効な鍵のない参照値と、参照処理そのものの利用不能を区別する。
            status = KeyResponseStatus::UnknownToken;
            break;
        default:
            break;
    }
    auto response = static_cast<uint8_t>(status);
    iovec data{&response, sizeof(response)};
    msghdr message{};
    message.msg_iov = &data;
    message.msg_iovlen = 1;
    alignas(cmsghdr) char control[CMSG_SPACE(2 * sizeof(int))]{};
    if (result == Result::Ok) {
        message.msg_control = control;
        message.msg_controllen = sizeof(control);
        auto* rights = CMSG_FIRSTHDR(&message);
        rights->cmsg_level = SOL_SOCKET;
        rights->cmsg_type = SCM_RIGHTS;
        rights->cmsg_len = CMSG_LEN(2 * sizeof(int));
        const int descriptors[] = {slotFd, ownerFd_};
        std::memcpy(CMSG_DATA(rights), descriptors, sizeof(descriptors));
    }
    if (waitSocket(fd, POLLOUT)) sendmsg(fd, &message, MSG_NOSIGNAL);
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
