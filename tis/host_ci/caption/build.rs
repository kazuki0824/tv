fn main() {
    println!("cargo:rerun-if-env-changed=ARIBCAPTION_LIB_DIR");
    if let Some(directory) = std::env::var_os("ARIBCAPTION_LIB_DIR") {
        println!(
            "cargo:rustc-link-search=native={}",
            std::path::Path::new(&directory).display()
        );
        for dependency in ["fontconfig", "freetype", "stdc++"] {
            println!("cargo:rustc-link-lib=dylib={dependency}");
        }
        return;
    }

    cc::Build::new()
        .file("caption_stub/aribcaption_stub.c")
        .warnings(true)
        .compile("aribcaption");
    println!("cargo:rerun-if-changed=caption_stub/aribcaption_stub.c");
}
