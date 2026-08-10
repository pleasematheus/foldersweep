fn main() {
    slint_build::compile("ui/app.slint").unwrap();
    println!("cargo:rerun-if-changed=ui/app.ico");

    // `#[cfg(target_os = "windows")]` aqui testaria o host, não o alvo:
    // build scripts são compilados para a máquina que roda o build. Num
    // cross-compile a partir do WSL o recurso de ícone sumiria em silêncio.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // O nome do arquivo resolve o Explorer, mas não o Gerenciador de
        // Tarefas nem a aba Detalhes das propriedades — esses leem o recurso
        // VERSIONINFO. Sem preencher, o winresource deriva tudo de
        // `CARGO_PKG_*` e a coluna "Nome" fica com a crate em minúsculas.
        winresource::WindowsResource::new()
            .set_icon("ui/app.ico")
            .set("ProductName", "Folder Sweep")
            .set("FileDescription", "Folder Sweep")
            .set("InternalName", "FolderSweep")
            .set("OriginalFilename", "FolderSweep.exe")
            .compile()
            .unwrap();
    }
}
