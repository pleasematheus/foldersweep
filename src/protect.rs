use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

/// Chave de comparação de caminho.
///
/// NTFS é case-insensitive, então `C:\Users\Matheus\Downloads` e
/// `c:\users\matheus\downloads` são a mesma pasta. Comparar byte a byte
/// deixaria um Known Folder escapar da classe A por diferença de caixa — e
/// um escape aqui significa apagar uma pasta que o sistema esperava.
fn fold(path: &Path) -> String {
    path.to_string_lossy().to_lowercase()
}

fn eq_ci(a: &OsStr, b: &OsStr) -> bool {
    a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
}

/// `Path::starts_with` case-insensitive.
///
/// Compara componente a componente em vez de prefixo de string, senão
/// `C:\Windows2` casaria com `C:\Windows`.
fn starts_with_ci(path: &Path, prefix: &Path) -> bool {
    let mut components = path.components();
    for expected in prefix.components() {
        match components.next() {
            Some(actual) if eq_ci(actual.as_os_str(), expected.as_os_str()) => {}
            _ => return false,
        }
    }
    true
}

/// Classe A: não apagar esta pasta, mas varrer dentro dela.
pub struct ExactGuard {
    paths: HashSet<String>,
}

impl ExactGuard {
    /// Known Folders resolvem uma vez só, não uma vez por raiz.
    pub fn new(volume_roots: &[PathBuf]) -> Self {
        let mut paths: HashSet<String> = volume_roots.iter().map(|p| fold(p)).collect();
        for known in known_folders() {
            paths.insert(fold(&known));
        }
        Self { paths }
    }

    pub fn is_protected(&self, path: &Path) -> bool {
        self.paths.contains(&fold(path))
    }

    #[cfg(test)]
    pub fn with_paths(paths: impl IntoIterator<Item = PathBuf>) -> Self {
        Self {
            paths: paths.into_iter().map(|p| fold(&p)).collect(),
        }
    }
}

fn known_folders() -> Vec<PathBuf> {
    [
        dirs::desktop_dir(),
        dirs::document_dir(),
        dirs::download_dir(),
        dirs::audio_dir(),
        dirs::video_dir(),
        dirs::picture_dir(),
        dirs::template_dir(),
        dirs::public_dir(),
        dirs::home_dir(),
        dirs::config_dir(),
        dirs::data_dir(),
        dirs::data_local_dir(),
        dirs::cache_dir(),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Raízes de sistema resolvidas do ambiente, **não** hardcoded em `C:`.
///
/// Windows não mora obrigatoriamente em C: — dual boot, instalação
/// relocada e Windows-To-Go colocam o SO noutra letra. Com `C:\Windows`
/// fixo, marcar esse outro disco na árvore faria a varredura descer no SO
/// e apagar diretórios vazios que instaladores e o servicing esperam.
/// Falha fechada: o conjunto é a **união** do ambiente com os caminhos
/// clássicos em `C:`. Só o ambiente falharia aberto — se `SystemRoot` viesse
/// vazio ou ausente (ambiente enxuto, processo lançado por um serviço, env
/// adulterado), a classe B simplesmente não cobriria o diretório do Windows
/// e a varredura desceria nele em silêncio. Só o hardcoded falharia aberto
/// na máquina com Windows noutra letra. A união nunca protege de menos.
fn system_roots() -> &'static [PathBuf] {
    static ROOTS: OnceLock<Vec<PathBuf>> = OnceLock::new();
    ROOTS.get_or_init(|| {
        const FROM_ENV: &[&str] = &[
            "SystemRoot",
            "windir",
            "ProgramFiles",
            "ProgramFiles(x86)",
            "ProgramW6432",
            "ProgramData",
        ];
        const FALLBACK: &[&str] = &[
            r"C:\Windows",
            r"C:\Program Files",
            r"C:\Program Files (x86)",
            r"C:\ProgramData",
        ];

        let mut roots: Vec<PathBuf> = FROM_ENV
            .iter()
            .filter_map(|key| std::env::var_os(key))
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .collect();

        for path in FALLBACK {
            let path = PathBuf::from(path);
            if !roots.iter().any(|r| fold(r) == fold(&path)) {
                roots.push(path);
            }
        }
        roots
    })
}

/// Classe B: podar a subárvore inteira, nunca descer.
///
/// Pressupõe que quem chama já descartou reparse points — tanto o walk da
/// varredura quanto a árvore da UI filtram `is_symlink()` antes de chegar
/// aqui. Repetir a checagem custaria um `symlink_metadata` por diretório
/// visitado, que numa varredura de disco inteiro é milhões de syscalls
/// para um ramo que nunca dispara.
pub fn is_subtree_protected(path: &Path) -> bool {
    // Varre todos os componentes em vez de só `file_name`: mais defensivo
    // se algum dia esta função for chamada com um caminho interno de `.git`
    // sem passar pela poda do walk.
    if path
        .components()
        .any(|c| matches!(c, Component::Normal(name) if eq_ci(name, OsStr::new(".git"))))
    {
        return true;
    }

    if system_roots().iter().any(|root| starts_with_ci(path, root)) {
        return true;
    }

    const SPECIAL_NAMES: &[&str] = &["$RECYCLE.BIN", "System Volume Information"];
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| SPECIAL_NAMES.iter().any(|s| s.eq_ignore_ascii_case(n)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_folder_is_exact_protected() {
        let Some(downloads) = dirs::download_dir() else {
            return;
        };
        let guard = ExactGuard::new(&[PathBuf::from(r"C:\")]);
        assert!(guard.is_protected(&downloads));
    }

    #[test]
    fn known_folder_matches_regardless_of_case() {
        let Some(downloads) = dirs::download_dir() else {
            return;
        };
        let guard = ExactGuard::new(&[PathBuf::from(r"C:\")]);
        let shouted = PathBuf::from(downloads.to_string_lossy().to_uppercase());
        assert!(guard.is_protected(&shouted));
    }

    #[test]
    fn homonym_outside_known_folder_is_not_protected() {
        let guard = ExactGuard::new(&[PathBuf::from(r"D:\")]);
        let fake = Path::new(r"D:\Projetos\app\Documents");
        assert!(!guard.is_protected(fake));
    }

    #[test]
    fn empty_folder_inside_known_folder_is_not_exact_protected() {
        let Some(downloads) = dirs::download_dir() else {
            return;
        };
        let guard = ExactGuard::new(&[PathBuf::from(r"C:\")]);
        let inside = downloads.join("zip-extraido").join("vazia");
        assert!(!guard.is_protected(&inside));
        assert!(!is_subtree_protected(&inside));
    }

    #[test]
    fn dot_git_subtree_is_subtree_protected() {
        assert!(is_subtree_protected(Path::new(r"D:\repo\.git\refs\tags")));
        assert!(is_subtree_protected(Path::new(r"D:\repo\.GIT\refs")));
    }

    #[test]
    fn volume_root_is_exact_protected() {
        let guard = ExactGuard::new(&[PathBuf::from(r"D:\")]);
        assert!(guard.is_protected(Path::new(r"D:\")));
    }

    #[test]
    fn all_selected_roots_are_exact_protected() {
        let guard = ExactGuard::new(&[PathBuf::from(r"C:\"), PathBuf::from(r"D:\")]);
        assert!(guard.is_protected(Path::new(r"C:\")));
        assert!(guard.is_protected(Path::new(r"D:\")));
    }

    #[test]
    fn system_roots_come_from_environment_not_hardcoded_c() {
        let roots = system_roots();
        assert!(
            !roots.is_empty(),
            "ambiente Windows deve expor SystemRoot/ProgramFiles"
        );

        // Qualquer que seja a letra em que o Windows está instalado, a
        // subárvore dele é protegida.
        let windir = std::env::var_os("SystemRoot")
            .or_else(|| std::env::var_os("windir"))
            .map(PathBuf::from)
            .expect("SystemRoot definido no Windows");
        assert!(is_subtree_protected(
            &windir.join("System32").join("config")
        ));
    }

    #[test]
    fn system_subtree_match_is_case_insensitive_and_component_wise() {
        let windir = std::env::var_os("SystemRoot")
            .or_else(|| std::env::var_os("windir"))
            .map(PathBuf::from)
            .expect("SystemRoot definido no Windows");

        let shouted = PathBuf::from(windir.to_string_lossy().to_uppercase());
        assert!(is_subtree_protected(&shouted.join("System32")));

        // Irmão com prefixo textual em comum não pode casar: a comparação é
        // por componente, não por prefixo de string.
        let sibling = PathBuf::from(format!("{}2", windir.to_string_lossy()));
        assert!(!is_subtree_protected(&sibling.join("config")));

        assert!(!is_subtree_protected(Path::new(r"D:\Projetos\Windows")));
    }

    #[test]
    fn classic_c_paths_stay_protected_even_if_environment_is_stripped() {
        // A união com os caminhos clássicos existe para falhar fechada: sem
        // ela, um ambiente sem SystemRoot deixaria o diretório do Windows
        // descoberto sem nenhum sinal.
        assert!(is_subtree_protected(Path::new(r"C:\Windows\System32")));
        assert!(is_subtree_protected(Path::new(r"C:\Program Files\App")));
        assert!(is_subtree_protected(Path::new(r"C:\ProgramData\App")));
        assert!(!is_subtree_protected(Path::new(
            r"C:\Users\alguem\Projetos"
        )));
    }

    #[test]
    fn starts_with_ci_respects_component_boundaries() {
        assert!(starts_with_ci(
            Path::new(r"C:\Windows\System32"),
            Path::new(r"c:\windows")
        ));
        assert!(!starts_with_ci(
            Path::new(r"C:\Windows2\System32"),
            Path::new(r"C:\Windows")
        ));
    }
}
