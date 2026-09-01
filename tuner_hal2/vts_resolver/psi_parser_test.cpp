#include "psi_parser.h"
#include <gtest/gtest.h>
using namespace tuner_hal2_vts;

TEST(PsiParser, ParsesPat) {
    std::vector<uint8_t> s = {0x00,0xb0,0x0d, 0x00,0x01, 0xc1,0x00,0x00, 0x00,0x64,0xe1,0x00, 0,0,0,0};
    auto programs = parse_pat(s);
    ASSERT_TRUE(programs.has_value());
    ASSERT_EQ(programs->size(), 1u);
    EXPECT_EQ((*programs)[0].service_id, 100);
    EXPECT_EQ((*programs)[0].pmt_pid, 0x100);
}

TEST(PsiParser, ParsesPmt) {
    std::vector<uint8_t> s = {0x02,0xb0,0x17, 0x00,0x64, 0xc1,0x00,0x00, 0xe1,0x01, 0xf0,0x00,
                              0x1b,0xe1,0x01,0xf0,0x00, 0x0f,0xe1,0x02,0xf0,0x00, 0,0,0,0};
    auto pmt = parse_pmt(s);
    ASSERT_TRUE(pmt.has_value());
    ASSERT_EQ(pmt->streams.size(), 2u);
    EXPECT_TRUE(is_video_stream_type(pmt->streams[0].stream_type));
    EXPECT_TRUE(is_audio_stream_type(pmt->streams[1].stream_type));
}
