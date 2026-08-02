fn main() {
    slint_build::compile("ui/app.slint").unwrap();
    println!("cargo:rerun-if-changed=ui/app.ico");

    // `#[cfg(target_os = "windows")]` aqui testaria o host, não o alvo:
    // build scripts são compilados para a máquina que roda o build. Num
    // cross-compile a partir do WSL o recurso de ícone sumiria em silêncio.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("ui/app.ico")
            .compile()
            .unwrap();
    }
}
