//! 実TSを製品demuxへ注入するホスト試験専用実行器。
use maleicacid_tuner_hal2_common::TsPacketCompletionBuffer;
use maleicacid_tuner_hal2_demux::{
    DemuxRuntime, FilterConfig, FilterConfigKind, FilterOpenType, FilterRuntimeConfigureRequest,
    FilterRuntimeOperationRequest, FilterRuntimeRegistrationRequest, OpenFilterRequest,
    PipelineDiagnostic, PipelineGeneratedEvent, SectionCondition, SectionConditionKind,
    TsInputOrigin, ValidatedPacketIngressRequest, ValidatedTsPacket,
};
use maleicacid_tuner_hal2_fmq::{host_ci_queue_snapshots, host_ci_reset_queue_registry};
use std::fs::File;
use std::io::{BufWriter, Read, Write};

fn checked<T, E: std::fmt::Debug>(result: Result<T, E>) -> Result<T, String> {
    result.map_err(|error| format!("{error:?}"))
}

struct Capture {
    runtime: DemuxRuntime,
    filters: Vec<(i32, i32)>,
    payloads: Vec<Vec<u8>>,
    output: Vec<u8>,
    packets: u32,
    sections: u32,
}

impl Capture {
    fn open(&mut self, pid: i32, table_id: u8) -> Result<(), String> {
        let id = checked(i32::try_from(self.filters.len() + 1))?;
        let request = OpenFilterRequest {
            open_type: FilterOpenType::TsSection,
            buffer_size: 65536,
            callback_present: true,
        };
        checked(self.runtime.register_filter_from_typed_request(
            FilterRuntimeRegistrationRequest::new(id, &request, 1024),
        ))?;
        self.filters.push((id, pid));
        self.payloads.push(Vec::new());
        let config = FilterConfig {
            open_type: FilterOpenType::TsSection,
            tpid: pid,
            kind: FilterConfigKind::TsSection {
                check_crc: true,
                repeat: true,
                raw: false,
                length_field_bits: 12,
                condition: SectionCondition {
                    kind: SectionConditionKind::SectionBits,
                    filter: vec![table_id],
                    mask: vec![255],
                    mode: vec![0],
                    table_id: None,
                    version: None,
                },
            },
        };
        checked(self.runtime.configure_filter_runtime_with_typed_request(
            FilterRuntimeConfigureRequest::new(id, config),
        ).1)?;
        checked(self.runtime.start_filter_runtime_from_typed_request(
            FilterRuntimeOperationRequest::new(id),
        ))
    }

    fn packet(&mut self, packet: &[u8]) -> Result<(), String> {
        let validated = checked(ValidatedTsPacket::validate(packet))?;
        let report = self.runtime.push_validated_ts_packet_from_typed_request(
            ValidatedPacketIngressRequest::new(&validated, TsInputOrigin::frontend(1)),
        );
        for diagnostic in &report.diagnostics {
            if !matches!(diagnostic, PipelineDiagnostic::NoPayloadAssemblySuppressed { .. }) {
                return Err(format!("packet {}: {diagnostic:?}", self.packets));
            }
        }
        if report.malformed_packets != 0 || report.dropped_packets != 0 {
            return Err(format!("packet {}: rejected", self.packets));
        }
        self.packets += 1;
        for event in report.generated_events {
            if let PipelineGeneratedEvent::SectionPayloadReady { filter_id, pid, raw, bytes, .. } = event {
                if raw { return Err("raw section".into()); }
                let index = self.filters.iter().position(|&(id, expected_pid)| {
                    id == filter_id && expected_pid == pid.to_i32_for_aidl_boundary()
                }).ok_or("unknown filter")?;
                self.payloads[index].extend_from_slice(&bytes);
                self.output.extend_from_slice(&self.sections.to_be_bytes());
                self.output.extend_from_slice(&pid.to_i32_for_aidl_boundary().to_be_bytes());
                self.output.extend_from_slice(&checked(u32::try_from(bytes.len()))?.to_be_bytes());
                self.output.extend_from_slice(&bytes);
                self.sections += 1;
                if self.output.len() > 1024 * 1024 { return Err("output limit".into()); }
            }
        }
        Ok(())
    }

    fn read(&mut self, input: &str) -> Result<usize, String> {
        let mut file = checked(File::open(input))?;
        let mut completion = TsPacketCompletionBuffer::default();
        let mut buffer = [0_u8; 4093];
        loop {
            let count = checked(file.read(&mut buffer))?;
            if count == 0 { break; }
            let drain = completion.push(&buffer[..count]);
            if drain.malformed_bytes != 0 { return Err("malformed input".into()); }
            for packet in drain.packets { self.packet(&packet)?; }
        }
        let drain = completion.drain_for_boundary();
        for packet in drain.packets { self.packet(&packet)?; }
        if host_ci_queue_snapshots() != self.payloads {
            return Err("queue/event mismatch".into());
        }
        Ok(drain.malformed_bytes)
    }

    fn close(&mut self) -> Vec<String> {
        let mut failures = Vec::new();
        for &(id, _) in self.filters.iter().rev() {
            if let Err(error) = self.runtime.stop_filter_runtime_with_typed_request(
                FilterRuntimeOperationRequest::new(id),
            ).1 { failures.push(format!("stop {id}: {error:?}")); }
            if let Err(error) = self.runtime.remove_filter_from_typed_request(
                FilterRuntimeOperationRequest::new(id),
            ) { failures.push(format!("remove {id}: {error:?}")); }
        }
        if !host_ci_queue_snapshots().is_empty() { failures.push("retained queue".into()); }
        failures
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let input = args.next().ok_or("input path required")?;
    let output = args.next().ok_or("output path required")?;
    host_ci_reset_queue_registry();
    let mut capture = Capture {
        runtime: DemuxRuntime::new(1, 1), filters: Vec::new(), payloads: Vec::new(),
        output: Vec::new(), packets: 0, sections: 0,
    };
    let primary = (|| {
        for pair in args {
            let (pid, table) = pair.split_once(':').ok_or("PID:table required")?;
            capture.open(checked(pid.parse())?, checked(table.parse())?)?;
        }
        if capture.filters.is_empty() { return Err("filters required".into()); }
        capture.read(&input)
    })();
    let cleanup = capture.close();
    if !cleanup.is_empty() { return Err(format!("primary={primary:?}, cleanup={cleanup:?}")); }
    let tail = primary?;
    // 終了報告はキュー照合と全フィルターの停止・解放に成功した後だけ出力する。
    for value in [u32::MAX, capture.packets, checked(u32::try_from(tail))?, capture.sections,
        checked(u32::try_from(capture.filters.len()))?] {
        capture.output.extend_from_slice(&value.to_be_bytes());
    }
    let mut writer = BufWriter::new(checked(File::create(output))?);
    checked(writer.write_all(&capture.output))?;
    checked(writer.flush())?;
    eprintln!("packets={}, sections={}, trailing_bytes={}, queues_verified_and_released={}",
        capture.packets, capture.sections, tail, capture.filters.len());
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => { eprintln!("{error}"); std::process::ExitCode::FAILURE }
    }
}
