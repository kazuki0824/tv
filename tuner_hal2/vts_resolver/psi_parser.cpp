#include "psi_parser.h"

#include <algorithm>

namespace tuner_hal2_vts {
namespace {
uint16_t pid_from(const uint8_t* p) {
    return static_cast<uint16_t>(((p[0] & 0x1f) << 8) | p[1]);
}
size_t section_total_size(const std::vector<uint8_t>& s) {
    if (s.size() < 3) return 0;
    const size_t section_length = static_cast<size_t>(((s[1] & 0x0f) << 8) | s[2]);
    return 3 + section_length;
}
}  // namespace

std::optional<std::vector<ProgramMap>> parse_pat(const std::vector<uint8_t>& section) {
    if (section.size() < 12 || section[0] != 0x00) return std::nullopt;
    const size_t total = section_total_size(section);
    if (total > section.size() || total < 12) return std::nullopt;
    std::vector<ProgramMap> programs;
    for (size_t off = 8; off + 4 <= total - 4; off += 4) {
        const uint16_t program = static_cast<uint16_t>((section[off] << 8) | section[off + 1]);
        const uint16_t pid = pid_from(&section[off + 2]);
        if (program != 0) programs.push_back({program, pid});
    }
    return programs;
}

std::optional<PmtInfo> parse_pmt(const std::vector<uint8_t>& section) {
    if (section.size() < 16 || section[0] != 0x02) return std::nullopt;
    const size_t total = section_total_size(section);
    if (total > section.size() || total < 16) return std::nullopt;
    PmtInfo info;
    info.service_id = static_cast<uint16_t>((section[3] << 8) | section[4]);
    const size_t program_info_length = static_cast<size_t>(((section[10] & 0x0f) << 8) | section[11]);
    size_t off = 12 + program_info_length;
    if (off > total - 4) return std::nullopt;
    while (off + 5 <= total - 4) {
        const uint8_t stream_type = section[off];
        const uint16_t pid = pid_from(&section[off + 1]);
        const size_t es_info_length = static_cast<size_t>(((section[off + 3] & 0x0f) << 8) | section[off + 4]);
        if (off + 5 + es_info_length > total - 4) return std::nullopt;
        info.streams.push_back({stream_type, pid});
        off += 5 + es_info_length;
    }
    return info;
}

bool is_video_stream_type(uint8_t t) {
    switch (t) {
        case 0x01: case 0x02: case 0x10: case 0x1b: case 0x24: return true;
        default: return false;
    }
}

bool is_audio_stream_type(uint8_t t) {
    switch (t) {
        case 0x03: case 0x04: case 0x0f: case 0x11: case 0x81: return true;
        default: return false;
    }
}

std::optional<std::vector<uint8_t>> SectionAssembler::append(const uint8_t* data, size_t size, bool new_section) {
    if (new_section) {
        section_.clear();
        expected_size_ = 0;
    }
    if (size == 0) return std::nullopt;
    section_.insert(section_.end(), data, data + size);
    if (expected_size_ == 0 && section_.size() >= 3) {
        if (section_[0] != expected_table_id_) {
            section_.clear();
            return std::nullopt;
        }
        expected_size_ = section_total_size(section_);
        if (expected_size_ < 8 || expected_size_ > 4096) {
            section_.clear();
            expected_size_ = 0;
            return std::nullopt;
        }
    }
    if (expected_size_ != 0 && section_.size() >= expected_size_) {
        section_.resize(expected_size_);
        auto done = section_;
        section_.clear();
        expected_size_ = 0;
        return done;
    }
    return std::nullopt;
}

std::optional<std::vector<uint8_t>> SectionAssembler::push_ts_packet(const uint8_t* packet, size_t size) {
    if (size != 188 || packet[0] != 0x47) return std::nullopt;
    const bool payload_unit_start = (packet[1] & 0x40) != 0;
    const uint8_t adaptation_control = static_cast<uint8_t>((packet[3] >> 4) & 0x03);
    if (adaptation_control == 0 || adaptation_control == 2) return std::nullopt;
    size_t off = 4;
    if (adaptation_control == 3) {
        if (off >= size) return std::nullopt;
        off += 1 + packet[off];
        if (off > size) return std::nullopt;
    }
    if (off >= size) return std::nullopt;
    if (payload_unit_start) {
        const uint8_t pointer = packet[off++];
        if (off + pointer > size) return std::nullopt;
        if (!section_.empty() && pointer != 0) {
            if (auto done = append(packet + off, pointer, false)) return done;
        }
        off += pointer;
        if (off >= size) return std::nullopt;
        return append(packet + off, size - off, true);
    }
    if (section_.empty()) return std::nullopt;
    return append(packet + off, size - off, false);
}

}  // namespace tuner_hal2_vts
