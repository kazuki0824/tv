#pragma once
#include <cstddef>
#include <cstdint>
#include <optional>
#include <utility>
#include <vector>

namespace tuner_hal2_vts {

struct ProgramMap {
    uint16_t service_id = 0;
    uint16_t pmt_pid = 0;
};

struct ElementaryStream {
    uint8_t stream_type = 0;
    uint16_t pid = 0;
};

struct PmtInfo {
    uint16_t service_id = 0;
    std::vector<ElementaryStream> streams;
};

std::optional<std::vector<ProgramMap>> parse_pat(const std::vector<uint8_t>& section);
std::optional<PmtInfo> parse_pmt(const std::vector<uint8_t>& section);
bool is_video_stream_type(uint8_t stream_type);
bool is_audio_stream_type(uint8_t stream_type);

class SectionAssembler {
  public:
    explicit SectionAssembler(uint8_t expected_table_id) : expected_table_id_(expected_table_id) {}
    std::optional<std::vector<uint8_t>> push_ts_packet(const uint8_t* packet, size_t size);

  private:
    std::optional<std::vector<uint8_t>> append(const uint8_t* data, size_t size, bool new_section);
    uint8_t expected_table_id_;
    std::vector<uint8_t> section_;
    size_t expected_size_ = 0;
};

}  // namespace tuner_hal2_vts
