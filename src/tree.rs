use std::path::{Path, PathBuf};

use crate::protect::is_subtree_protected;

pub struct Node {
    pub path: PathBuf,
    pub name: String,
    pub depth: i32,
    pub protected: bool,
}

/// Subdiretórios imediatos, ordenados por nome (case-insensitive).
///
/// Um nível por vez: a árvore carrega sob demanda porque enumerar um disco
/// inteiro de antemão é exatamente o que a barra indeterminada existe para
/// evitar. Erro de leitura e reparse point viram lista vazia — a árvore é
/// navegação, não varredura, e não tem onde reportar erro.
pub fn subdirectories(dir: &Path, depth: i32) -> Vec<Node> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut nodes: Vec<Node> = entries
        .flatten()
        .filter(|entry| {
            entry
                .file_type()
                .map(|t| t.is_dir() && !t.is_symlink())
                .unwrap_or(false)
        })
        .map(|entry| {
            let path = entry.path();
            Node {
                name: entry.file_name().to_string_lossy().into_owned(),
                protected: is_subtree_protected(&path),
                path,
                depth,
            }
        })
        .collect();

    nodes.sort_by_key(|node| node.name.to_lowercase());
    nodes
}

/// Quantas linhas depois de `index` são descendentes dele, dada a coluna de
/// profundidades da lista achatada. É o que define quanto remover ao colapsar.
pub fn descendant_count(depths: &[i32], index: usize) -> usize {
    let Some(&base) = depths.get(index) else {
        return 0;
    };
    depths[index + 1..]
        .iter()
        .take_while(|&&d| d > base)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("foldersweep-tree-{name}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn counts_only_descendants_not_later_siblings() {
        //  0: D:\           <- index 0
        //  1:   Projetos    <- descendente
        //  2:     app       <- descendente
        //  3:   Downloads   <- descendente
        //  4: E:\           <- irmão, para a contagem
        let depths = [0, 1, 2, 1, 0];
        assert_eq!(descendant_count(&depths, 0), 3);
        assert_eq!(descendant_count(&depths, 1), 1);
        assert_eq!(descendant_count(&depths, 2), 0);
        assert_eq!(descendant_count(&depths, 4), 0);
    }

    #[test]
    fn count_is_zero_for_out_of_range_index() {
        assert_eq!(descendant_count(&[0, 1], 5), 0);
    }

    #[test]
    fn lists_only_directories_sorted_by_name() {
        let root = temp_root("listagem");
        fs::create_dir_all(root.join("zebra")).unwrap();
        fs::create_dir_all(root.join("Alfa")).unwrap();
        fs::write(root.join("arquivo.txt"), b"x").unwrap();

        let nodes = subdirectories(&root, 1);
        let names: Vec<&str> = nodes.iter().map(|n| n.name.as_str()).collect();

        assert_eq!(names, vec!["Alfa", "zebra"]);
        assert!(nodes.iter().all(|n| n.depth == 1));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn marks_protected_subtrees() {
        let root = temp_root("protegida");
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(root.join("normal")).unwrap();

        let nodes = subdirectories(&root, 1);
        let git = nodes.iter().find(|n| n.name == ".git").unwrap();
        let normal = nodes.iter().find(|n| n.name == "normal").unwrap();

        assert!(git.protected);
        assert!(!normal.protected);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn unreadable_directory_yields_empty_instead_of_error() {
        let missing = temp_root("inexistente").join("nao-existe");
        assert!(subdirectories(&missing, 1).is_empty());
    }
}
