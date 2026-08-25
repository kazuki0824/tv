fn main() {
    cc::Build::new()
        .file("caption_stub/aribcaption_stub.c")
        .warnings(true)
        .compile("aribcaption");
    println!("cargo:rerun-if-changed=caption_stub/aribcaption_stub.c");
}
