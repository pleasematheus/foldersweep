use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::protect::{ExactGuard, is_subtree_protected};

const THROTTLE: Duration = Duration::from_millis(100);

#[derive(Default, Clone, Copy)]
pub struct SweepStats {
    pub scanned: u64,
    pub deleted: u64,
    pub errors: u64,
}

/// Varre todos os alvos selecionados em sequência, acumulando contadores e
/// tempo decorrido através da corrida inteira (não por alvo).
pub fn sweep_all<F>(roots: &[PathBuf], cancel: &AtomicBool, on_progress: F) -> SweepStats
where
    F: FnMut(SweepStats, &Path, Duration),
{
    // A ordem aqui importa e é fácil de escrever invertida.
    //
    // Guard vem da seleção COMPLETA: um alvo aninhado em outro ainda é uma
    // pasta que o usuário nomeou de propósito, e não pode ser removida só
    // porque o pai também está selecionado. Construir o guard a partir da
    // lista deduplicada tiraria `D:\Projetos` da classe A e ela sumiria
    // caso ficasse vazia durante a varredura de `D:\`.
    let guard = ExactGuard::new(roots);

    // Travessia vem da lista deduplicada: varrer `D:\` e `D:\Projetos`
    // percorreria a mesma subárvore duas vezes, inflando "analisadas" e
    // gastando tempo à toa.
    let walk_roots = dedupe_nested(roots);

    sweep_with_guard(&walk_roots, &guard, cancel, THROTTLE, on_progress)
}

/// Remove duplicatas exatas e alvos contidos em outro alvo já selecionado.
fn dedupe_nested(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut unique: Vec<PathBuf> = Vec::new();
    for root in roots {
        if !unique.contains(root) {
            unique.push(root.clone());
        }
    }
    unique
        .iter()
        .filter(|candidate| {
            // `starts_with` de `Path` compara por componente, então
            // `D:\Projetos2` não é considerado dentro de `D:\Projetos`.
            !unique
                .iter()
                .any(|other| other != *candidate && candidate.starts_with(other))
        })
        .cloned()
        .collect()
}

fn sweep_with_guard<F>(
    roots: &[PathBuf],
    guard: &ExactGuard,
    cancel: &AtomicBool,
    throttle: Duration,
    on_progress: F,
) -> SweepStats
where
    F: FnMut(SweepStats, &Path, Duration),
{
    let started = Instant::now();
    let mut walker = Walker {
        guard,
        cancel,
        throttle,
        started,
        last_report: started,
        stats: SweepStats::default(),
        on_progress,
    };

    // Sequencial, não paralelo: a varredura é limitada por I/O de disco e
    // paralelizar exigiria coordenar contadores entre threads sem ganho real.
    // ponytail: se varrer 4 discos ao mesmo tempo virar comum e lento,
    // uma thread por raiz com stats via Mutex é o próximo passo.
    for root in roots {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        walker.walk(root);
    }

    let stats = walker.stats;
    (walker.on_progress)(stats, Path::new(""), started.elapsed());
    stats
}

struct Walker<'a, F: FnMut(SweepStats, &Path, Duration)> {
    guard: &'a ExactGuard,
    cancel: &'a AtomicBool,
    throttle: Duration,
    started: Instant,
    last_report: Instant,
    stats: SweepStats,
    on_progress: F,
}

/// Um diretório aberto na pilha de travessia.
///
/// Guarda o `ReadDir` em vez de materializar os filhos num `Vec`: um
/// diretório com centenas de milhares de entradas não vira alocação de uma
/// vez só. É o mesmo que a versão recursiva fazia, um handle por nível.
struct Frame {
    path: PathBuf,
    entries: std::fs::ReadDir,
}

impl<F: FnMut(SweepStats, &Path, Duration)> Walker<'_, F> {
    /// Pós-ordem iterativo, com pilha explícita no heap.
    ///
    /// Não é `WalkDir::contents_first`: a combinação `contents_first` +
    /// `filter_entry` do walkdir 2.5 corrompe o walk — `skip_current_dir`
    /// faz pop() na pilha de leitura atual, que em modo pós-ordem pode já
    /// pertencer a um diretório-irmão não relacionado ao rejeitado.
    ///
    /// E não é recursivo: recursão por diretório transforma profundidade de
    /// árvore em profundidade de pilha. Com long paths, o NTFS aceita ~32k
    /// caracteres de caminho, e cada quadro carrega dois `WIN32_FIND_DATAW`
    /// (~600 bytes cada), então alguns milhares de níveis estouram os 2 MiB
    /// da worker. Estouro de pilha em Rust é abort imediato, sem unwinding.
    /// Não corromperia nada — cada remoção é atômica e independente — mas
    /// mataria o processo no meio da varredura. Com a pilha no heap, a
    /// profundidade passa a custar só memória.
    fn walk(&mut self, root: &Path) {
        let entries = match std::fs::read_dir(root) {
            Ok(entries) => entries,
            Err(_) => {
                self.stats.errors += 1;
                return;
            }
        };
        let mut stack = vec![Frame {
            path: root.to_path_buf(),
            entries,
        }];

        while !stack.is_empty() {
            if self.cancel.load(Ordering::Relaxed) {
                return;
            }

            // Borrow escopado a esta linha só, para a pilha ficar livre para
            // push/pop no resto do corpo.
            let next = stack
                .last_mut()
                .expect("pilha não vazia: verificado na condição do while")
                .entries
                .next();

            let Some(entry) = next else {
                // Diretório esgotado: os filhos já foram tratados, agora é a
                // vez dele. A raiz não é removida — ela sai da pilha sem
                // ninguém abaixo para chamar `finish_directory`.
                let done = stack.pop().expect("pilha não vazia");
                if !stack.is_empty() {
                    self.finish_directory(&done.path);
                }
                continue;
            };

            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    self.stats.errors += 1;
                    continue;
                }
            };
            let path = entry.path();

            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) => {
                    self.stats.errors += 1;
                    continue;
                }
            };

            // Arquivo, ou reparse point (junction/symlink) — nunca percorrido
            // nem removido, mesmo que o atributo de diretório também esteja
            // setado (caso das junctions no Windows).
            if !file_type.is_dir() || file_type.is_symlink() {
                continue;
            }

            // Conta antes da poda: uma pasta podada foi analisada, só não
            // foi descida.
            self.stats.scanned += 1;

            // Classe B: poda antes de descer.
            if is_subtree_protected(&path) {
                continue;
            }

            match std::fs::read_dir(&path) {
                Ok(entries) => stack.push(Frame { path, entries }),
                Err(_) => {
                    // Ilegível. Conta o erro e mesmo assim tenta remover, que
                    // é o que a versão recursiva fazia ao retornar da chamada:
                    // uma pasta pode estar vazia e sem permissão de leitura.
                    self.stats.errors += 1;
                    self.finish_directory(&path);
                }
            }
        }
    }

    /// Chamado quando um diretório já teve todo o conteúdo tratado.
    fn finish_directory(&mut self, path: &Path) {
        // Classe A: varre por dentro (já foi feito), nunca remove ela mesma.
        if !self.guard.is_protected(path) {
            match std::fs::remove_dir(path) {
                Ok(()) => self.stats.deleted += 1,
                Err(e) if e.kind() == std::io::ErrorKind::DirectoryNotEmpty => {}
                Err(_) => self.stats.errors += 1,
            }
        }

        // ponytail: o relógio só avança quando o walk emite progresso, então
        // um read_dir muito lento congela o contador de tempo por alguns
        // segundos (a barra indeterminada continua animando, então o sinal
        // de "não travei" sobrevive). Se isso incomodar na prática, trocar
        // por um slint::Timer na thread de UI.
        if self.last_report.elapsed() >= self.throttle {
            (self.on_progress)(self.stats, path, self.started.elapsed());
            self.last_report = Instant::now();
        }
    }
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
        let dir = std::env::temp_dir().join(format!("foldersweep-test-{name}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn cascades_in_one_pass_and_respects_guards() {
        let root = temp_root("cascade");

        // A/B/C aninhada, tudo vazio -> cascata inteira numa passada.
        fs::create_dir_all(root.join("A/B/C")).unwrap();

        // Pasta com arquivo sobrevive.
        fs::create_dir_all(root.join("com-arquivo")).unwrap();
        fs::write(root.join("com-arquivo/nota.txt"), b"x").unwrap();

        // Vacuidade estrita: desktop.ini sozinho não é vazio.
        fs::create_dir_all(root.join("com-desktop-ini")).unwrap();
        fs::write(root.join("com-desktop-ini/desktop.ini"), b"").unwrap();

        // Classe A simulada: pasta protegida por caminho exato, mas
        // varrida por dentro — o filho vazio some, ela sobrevive.
        let exact_protected = root.join("protegida-exata");
        fs::create_dir_all(exact_protected.join("filho-vazio")).unwrap();

        // Classe B simulada: subárvore .git nunca é percorrida.
        fs::create_dir_all(root.join(".git/refs/tags")).unwrap();

        let guard = ExactGuard::with_paths([exact_protected.clone()]);
        let cancel = AtomicBool::new(false);
        let roots = [root.clone()];
        let stats = sweep_with_guard(&roots, &guard, &cancel, Duration::ZERO, |_, _, _| {});

        assert!(!root.join("A").exists());
        assert!(root.join("com-arquivo").exists());
        assert!(root.join("com-desktop-ini/desktop.ini").exists());
        assert!(exact_protected.exists());
        assert!(!exact_protected.join("filho-vazio").exists());
        assert!(root.join(".git/refs/tags").exists());
        assert_eq!(stats.deleted, 4); // C, B, A, filho-vazio
        assert_eq!(stats.errors, 0);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn cancel_stops_and_prior_deletions_persist() {
        let root = temp_root("cancel");
        for i in 0..20 {
            fs::create_dir_all(root.join(format!("d{i}"))).unwrap();
        }

        let guard = ExactGuard::with_paths([]);
        let cancel = AtomicBool::new(false);
        let roots = [root.clone()];
        let mut seen = 0u32;
        let stats = sweep_with_guard(&roots, &guard, &cancel, Duration::ZERO, |_, _, _| {
            seen += 1;
            if seen == 5 {
                cancel.store(true, Ordering::Relaxed);
            }
        });

        assert!(stats.deleted > 0);
        assert!(stats.deleted < 20);

        let remaining = fs::read_dir(&root).unwrap().count() as u64;
        assert_eq!(remaining, 20 - stats.deleted);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn subtree_is_never_descended() {
        let root = temp_root("subtree-prune");
        fs::create_dir_all(root.join(".git/objects/aa")).unwrap();

        let guard = ExactGuard::with_paths([]);
        let cancel = AtomicBool::new(false);
        let roots = [root.clone()];
        let stats = sweep_with_guard(&roots, &guard, &cancel, Duration::ZERO, |_, _, _| {});

        assert!(root.join(".git/objects/aa").exists());
        assert_eq!(stats.deleted, 0);
        // .git foi analisada (contada) mas não descida: objects/ e aa/ nunca
        // aparecem no contador.
        assert_eq!(stats.scanned, 1);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn multiple_roots_accumulate_stats() {
        let a = temp_root("multi-a");
        let b = temp_root("multi-b");
        fs::create_dir_all(a.join("vazia1")).unwrap();
        fs::create_dir_all(a.join("vazia2")).unwrap();
        fs::create_dir_all(b.join("vazia3")).unwrap();

        let guard = ExactGuard::with_paths([]);
        let cancel = AtomicBool::new(false);
        let roots = [a.clone(), b.clone()];
        let stats = sweep_with_guard(&roots, &guard, &cancel, Duration::ZERO, |_, _, _| {});

        // Contadores somam as duas raízes em vez de zerar na segunda.
        assert_eq!(stats.deleted, 3);
        assert_eq!(stats.scanned, 3);
        assert_eq!(stats.errors, 0);

        fs::remove_dir_all(&a).ok();
        fs::remove_dir_all(&b).ok();
    }

    #[test]
    fn dedupe_drops_nested_and_duplicate_roots() {
        let roots = vec![
            PathBuf::from(r"D:\"),
            PathBuf::from(r"D:\Projetos"),
            PathBuf::from(r"E:\"),
            PathBuf::from(r"E:\"),
        ];
        assert_eq!(
            dedupe_nested(&roots),
            vec![PathBuf::from(r"D:\"), PathBuf::from(r"E:\")]
        );
    }

    #[test]
    fn dedupe_keeps_sibling_with_shared_name_prefix() {
        // Comparação é por componente, não por string: Projetos2 não está
        // dentro de Projetos.
        let roots = vec![
            PathBuf::from(r"D:\Projetos"),
            PathBuf::from(r"D:\Projetos2"),
        ];
        assert_eq!(dedupe_nested(&roots).len(), 2);
    }

    #[test]
    fn nested_selected_target_survives_even_though_walk_is_deduped() {
        let root = temp_root("alvo-aninhado");
        let inner = root.join("alvo-interno");
        fs::create_dir_all(inner.join("vazia")).unwrap();

        let cancel = AtomicBool::new(false);
        // Usuário selecionou o pai E o filho. O walk roda só o pai, mas o
        // guard tem que conhecer os dois.
        let roots = [root.clone(), inner.clone()];
        sweep_all(&roots, &cancel, |_, _, _| {});

        assert!(
            !inner.join("vazia").exists(),
            "pasta vazia comum deve sumir"
        );
        assert!(inner.exists(), "alvo nomeado pelo usuário deve sobreviver");
        assert!(root.exists());

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn deep_nesting_does_not_exhaust_the_stack() {
        let root = temp_root("profunda");
        // Prefixo verbatim: sem ele o Windows corta em MAX_PATH (260) e a
        // árvore não chegaria fundo o bastante para o teste significar algo.
        let deep_root = PathBuf::from(format!(r"\\?\{}", root.display()));

        const DEPTH: usize = 1500;
        let mut path = deep_root.clone();
        for _ in 0..DEPTH {
            path.push("d");
        }
        fs::create_dir_all(&path).unwrap();

        let guard = ExactGuard::with_paths([]);
        let cancel = AtomicBool::new(false);
        let roots = [deep_root.clone()];
        let stats = sweep_with_guard(&roots, &guard, &cancel, Duration::ZERO, |_, _, _| {});

        // Cascata inteira numa passada, sem estourar a pilha. Com quadros
        // recursivos (~1,2 KB cada por causa dos dois WIN32_FIND_DATAW),
        // 1500 níveis ficam na borda dos 2 MiB da thread.
        assert_eq!(stats.deleted, DEPTH as u64);
        assert_eq!(stats.errors, 0);
        assert!(!deep_root.join("d").exists());
        assert!(root.exists(), "a raiz nunca é removida");

        fs::remove_dir_all(&deep_root).ok();
    }

    #[test]
    fn roots_themselves_are_never_removed() {
        let root = temp_root("root-survives");
        let guard = ExactGuard::new(std::slice::from_ref(&root));
        let cancel = AtomicBool::new(false);
        let roots = [root.clone()];
        sweep_with_guard(&roots, &guard, &cancel, Duration::ZERO, |_, _, _| {});

        assert!(root.exists());
        fs::remove_dir_all(&root).ok();
    }
}
