#include <android_media_tuner_testing_configuration_V1_0.h>

int main(int argc, char** argv) {
    if (argc != 2) {
        return 2;
    }
    using namespace android::media::tuner::testing::configuration::V1_0;
    return read(argv[1]).has_value() ? 0 : 1;
}
