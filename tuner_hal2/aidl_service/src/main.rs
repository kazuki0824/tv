fn main() {
    logger::init(
        logger::Config::default()
            .with_tag_on_device("maleicacid-tuner-hal2")
            .with_max_level(log::LevelFilter::Info),
    );
    maleicacid_tuner_hal2_aidl_service::run_service();
}
