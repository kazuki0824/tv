use maleicacid_arib_si_engine_core::runtime_snapshot_dto::{
    BroadcastSystemDto, BulkSnapshotDto, CaDescriptorScopeDto, CaMetadataSourceDto,
    EitTimingStateDto, ElementaryStreamKindDto, SiParseStatusDto, SmdSemanticStateDto,
};
use serde_generate::{kotlin, CodeGeneratorConfig, SourceInstaller};
use serde_reflection::{Tracer, TracerConfig};
use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let output_root = env::args()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: maleicacid-si-kotlin-bindings <kotlin-source-root>")?;

    let mut tracer = Tracer::new(TracerConfig::default());
    tracer.trace_simple_type::<BulkSnapshotDto>()?;
    tracer.trace_simple_type::<SiParseStatusDto>()?;
    tracer.trace_simple_type::<EitTimingStateDto>()?;
    tracer.trace_simple_type::<ElementaryStreamKindDto>()?;
    tracer.trace_simple_type::<BroadcastSystemDto>()?;
    tracer.trace_simple_type::<SmdSemanticStateDto>()?;
    tracer.trace_simple_type::<CaDescriptorScopeDto>()?;
    tracer.trace_simple_type::<CaMetadataSourceDto>()?;
    let registry = tracer.registry()?;

    let config = CodeGeneratorConfig::new("com.maleicacid.tvinput.aribsi.generated".to_string())
        .with_serialization(false);
    let generated_dir = output_root.join("com/maleicacid/tvinput/aribsi/generated");
    let installer = kotlin::Installer::new(output_root);
    installer.install_module(&config, &registry)?;
    append_repository_trailing_blank_line(&generated_dir)?;
    Ok(())
}

fn append_repository_trailing_blank_line(dir: &Path) -> Result<(), Box<dyn Error>> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("kt") {
            continue;
        }
        let mut content = fs::read_to_string(&path)?;
        if !content.ends_with("\n\n") {
            content.push('\n');
            fs::write(path, content)?;
        }
    }
    Ok(())
}
