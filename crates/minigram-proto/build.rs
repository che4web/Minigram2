fn main() {
    println!("cargo:rerun-if-changed=proto/minigram.proto");
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(&["proto/minigram.proto"], &["proto"])
        .expect("failed to compile protos");
}
