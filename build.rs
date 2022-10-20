use std::io::Result;

fn main() -> Result<()> {
    tonic_build::configure()
        .build_server(false)
        .build_client(true)
        .compile(&["proto/management.proto"], &["proto"])?;

    tonic_build::configure()
        .build_server(false)
        .build_client(true)
        .compile(&["proto/directory.proto"], &["proto"])?;

    Ok(())
}
