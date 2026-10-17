use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from("proto");
    let mut files: Vec<PathBuf> = std::fs::read_dir(root.join("gz/msgs"))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|ext| ext == "proto"))
        .collect();
    files.sort();

    // protox is a pure-Rust protobuf compiler, so no `protoc` is needed to build.
    let fds = protox::compile(&files, [&root])?;
    prost_build::Config::new().compile_fds(fds)?;

    println!("cargo:rerun-if-changed=proto");
    Ok(())
}
