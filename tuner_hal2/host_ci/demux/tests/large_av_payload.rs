// 実TSから生成したAV eventを検査する。DMA-BUFのOS境界だけをhost fileで置換する。
use maleicacid_tuner_hal2_demux::{
    AvSettings, AvStreamKind, AvStreamTypeConfig, DemuxRuntime, FilterAvStreamTypeRuntimeRequest,
    FilterConfig, FilterConfigKind, FilterOpenType, FilterRuntimeConfigureRequest,
    FilterRuntimeOperationRequest, FilterRuntimeRegistrationRequest, OpenFilterRequest,
    PipelineGeneratedEvent, TsInputOrigin, ValidatedPacketIngressRequest, ValidatedTsPacket,
};
use std::os::fd::IntoRawFd;
use std::os::unix::fs::FileExt;
use std::sync::atomic::{AtomicUsize, Ordering};

#[no_mangle]
pub extern "C" fn tuner_dmabuf_heap_alloc_system(len: usize) -> i32 {
    static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "large-av-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed),
    ));
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)
        .expect("host allocation");
    std::fs::remove_file(path).unwrap();
    file.set_len(len as u64).unwrap();
    file.into_raw_fd()
}

#[test]
#[ignore = "CIで生成したAVC TSをLARGE_AV_TSへ指定する"]
fn encoded_large_access_unit_reaches_media_event() {
    let bytes = std::fs::read(std::env::var("LARGE_AV_TS").unwrap()).unwrap();
    let mut demux = DemuxRuntime::new(1, 1);
    let request = OpenFilterRequest {
        open_type: FilterOpenType::TsVideo,
        buffer_size: 4096,
        callback_present: true,
    };
    demux
        .register_filter_from_typed_request(FilterRuntimeRegistrationRequest::new(1, &request, 64))
        .unwrap();
    let (_, result) =
        demux.configure_filter_runtime_with_typed_request(FilterRuntimeConfigureRequest::new(
            1,
            FilterConfig {
                open_type: FilterOpenType::TsVideo,
                tpid: 0x281,
                kind: FilterConfigKind::TsAv(AvSettings {
                    is_passthrough: false,
                    is_secure_memory: false,
                }),
            },
        ));
    result.unwrap();
    demux
        .configure_filter_av_stream_type_from_typed_request(FilterAvStreamTypeRuntimeRequest::new(
            1,
            AvStreamTypeConfig {
                kind: AvStreamKind::Video,
                stream_type: 5,
            },
        ))
        .unwrap();
    demux
        .start_filter_runtime_from_typed_request(FilterRuntimeOperationRequest::new(1))
        .unwrap();
    let mut large = 0;
    for bytes in bytes.chunks_exact(188) {
        let packet = ValidatedTsPacket::validate(bytes).unwrap();
        let report = demux.push_validated_ts_packet_from_typed_request(
            ValidatedPacketIngressRequest::new(&packet, TsInputOrigin::frontend(1)),
        );
        for event in report.generated_events {
            if let PipelineGeneratedEvent::AvMedia { descriptor, .. } = event {
                if descriptor.data_length > 1024 * 1024 {
                    assert_eq!(descriptor.offset, 0);
                    assert!(descriptor.metadata.pts_90khz.is_some());
                    let mut payload = vec![0; descriptor.data_length];
                    descriptor
                        .event_local_file
                        .unwrap()
                        .read_exact_at(&mut payload, 0)
                        .unwrap();
                    let output = std::env::var("LARGE_AV_ES").unwrap();
                    let pts = descriptor.metadata.pts_90khz.unwrap();
                    println!("delivered AU bytes={} pts90k={pts}", payload.len());
                    std::fs::write(format!("{output}.pts"), pts.to_string()).unwrap();
                    std::fs::write(output, payload).unwrap();
                    large += 1;
                }
            }
        }
    }
    assert!(
        large > 0,
        "1 MiB超の符号化AUが実producerとallocatorを通ること"
    );
}
