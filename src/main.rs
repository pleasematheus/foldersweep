#![windows_subsystem = "windows"]

mod disks;
mod log;
mod protect;
mod sweep;
mod tree;

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use slint::{Model, ModelRc, SharedString, VecModel};

use crate::log::{LogOutcome, SweepLog, format_elapsed};

slint::include_modules!();

type Rows = Rc<VecModel<TreeRow>>;
/// Fonte de verdade da seleção. Fica fora do modelo de linhas de propósito:
/// colapsar um nó remove as linhas dos filhos, e se a seleção morasse só na
/// linha ela seria perdida. Aqui ela sobrevive a colapsar e reexpandir.
type Selection = Rc<RefCell<HashSet<PathBuf>>>;

fn main() -> Result<(), slint::PlatformError> {
    let window = AppWindow::new()?;

    let rows: Rows = Rc::new(VecModel::from(
        disks::list_disks()
            .iter()
            .map(|disk| TreeRow {
                path: disk.mount_point.display().to_string().into(),
                name: disk.mount_point.display().to_string().into(),
                detail: disk.label.clone().into(),
                size_text: disk.size_text.clone().into(),
                depth: 0,
                expandable: true,
                expanded: false,
                selected: false,
                protected: false,
                removable: disk.removable,
            })
            .collect::<Vec<_>>(),
    ));
    window.set_rows(ModelRc::from(rows.clone()));

    let selection: Selection = Rc::new(RefCell::new(HashSet::new()));
    let cancel_flag: Rc<RefCell<Option<Arc<AtomicBool>>>> = Rc::new(RefCell::new(None));

    window.on_toggle_expand({
        let rows = rows.clone();
        let selection = selection.clone();
        move |index| {
            let Ok(index) = usize::try_from(index) else {
                return;
            };
            let Some(row) = rows.row_data(index) else {
                return;
            };
            if row.expanded {
                collapse(&rows, index);
            } else {
                expand(&rows, index, &selection.borrow());
            }
        }
    });

    window.on_toggle_select({
        let window_weak = window.as_weak();
        let rows = rows.clone();
        let selection = selection.clone();
        move |index| {
            let Ok(index) = usize::try_from(index) else {
                return;
            };
            let Some(mut row) = rows.row_data(index) else {
                return;
            };
            if row.protected {
                return;
            }

            let path = PathBuf::from(row.path.as_str());
            let now_selected = {
                let mut selection = selection.borrow_mut();
                if selection.remove(&path) {
                    false
                } else {
                    selection.insert(path);
                    true
                }
            };
            row.selected = now_selected;
            rows.set_row_data(index, row);

            if let Some(window) = window_weak.upgrade() {
                window.set_selected_count(selection.borrow().len() as i32);
            }
        }
    });

    window.on_start({
        let window_weak = window.as_weak();
        let selection = selection.clone();
        let cancel_flag = cancel_flag.clone();
        move || {
            let window = window_weak.unwrap();

            let mut roots: Vec<PathBuf> = selection.borrow().iter().cloned().collect();
            if roots.is_empty() {
                return;
            }
            // HashSet não tem ordem; ordenar deixa a varredura determinística.
            roots.sort();

            // O log é aberto antes da primeira pasta, não depois da última:
            // ele é escrito conforme a varredura anda. Isso puxa a escolha do
            // destino para cá, antes de qualquer remoção.
            let started_at = log::now_text();
            let sweep_log = match open_log_if_requested(&window, &roots, &started_at) {
                Ok(log) => log,
                Err(status) => {
                    // Nada foi apagado ainda. Sem log utilizável, a varredura
                    // não começa: quem marcou a caixa pediu o registro, e
                    // apagar sem ele é o oposto do que foi pedido.
                    window.set_log_status(status.into());
                    return;
                }
            };

            let cancel = Arc::new(AtomicBool::new(false));
            *cancel_flag.borrow_mut() = Some(cancel.clone());

            window.set_running(true);
            window.set_finished(false);
            window.set_scanned_count(0);
            window.set_deleted_count(0);
            window.set_error_count(0);
            window.set_elapsed_text("0:00".into());
            window.set_current_path(SharedString::new());
            window.set_log_status(SharedString::new());

            let window_weak = window_weak.clone();
            std::thread::spawn(move || {
                let started = std::time::Instant::now();
                let progress_weak = window_weak.clone();
                let (stats, sweep_log) =
                    sweep::sweep_all(&roots, &cancel, sweep_log, move |stats, path, elapsed| {
                        let window_weak = progress_weak.clone();
                        let path_text: SharedString = path.display().to_string().into();
                        let elapsed_text: SharedString = format_elapsed(elapsed).into();
                        let scanned = stats.scanned as i32;
                        let deleted = stats.deleted as i32;
                        let errors = stats.errors as i32;
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(window) = window_weak.upgrade() {
                                window.set_scanned_count(scanned);
                                window.set_deleted_count(deleted);
                                window.set_error_count(errors);
                                window.set_elapsed_text(elapsed_text);
                                window.set_current_path(path_text);
                            }
                        });
                    });

                let log_status = match sweep_log.finish(
                    stats,
                    started.elapsed(),
                    cancel.load(Ordering::Relaxed),
                    &log::now_text(),
                ) {
                    LogOutcome::Disabled => String::new(),
                    LogOutcome::Written(path) => format!("Log gravado em {}", path.display()),
                    // A varredura terminou de verdade; só o registro dela é
                    // que ficou incompleto, e dizer isso é o mínimo.
                    LogOutcome::Failed(e) => format!("Log interrompido por erro de escrita: {e}"),
                };

                let scanned = stats.scanned as i32;
                let deleted = stats.deleted as i32;
                let errors = stats.errors as i32;
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(window) = window_weak.upgrade() {
                        window.set_running(false);
                        window.set_log_status(log_status.into());
                        // Marca que uma corrida aconteceu. Sem isso, uma
                        // varredura que analisou zero pastas (pasta folha
                        // marcada) não mostraria resumo nenhum, e "nunca
                        // rodou" ficaria indistinguível de "rodou e não
                        // achou nada".
                        window.set_finished(true);
                        window.set_scanned_count(scanned);
                        window.set_deleted_count(deleted);
                        window.set_error_count(errors);
                        window.set_current_path(SharedString::new());
                    }
                });
            });
        }
    });

    window.on_cancel({
        let cancel_flag = cancel_flag.clone();
        move || {
            if let Some(cancel) = cancel_flag.borrow().as_ref() {
                cancel.store(true, Ordering::Relaxed);
            }
        }
    });

    // Fechar a janela sinaliza cancelamento antes de o processo começar a
    // descer. Sem isso a worker continuava chamando remove_dir durante o
    // teardown, entre `run()` retornar e o processo de fato morrer — uma
    // janela curta, mas apagando de verdade, e justamente depois de o
    // usuário ter indicado que queria parar.
    //
    // ponytail: sinaliza e deixa fechar, sem join. A unidade de trabalho é
    // um remove_dir atômico, então morrer no meio não deixa estado parcial.
    window.window().on_close_requested({
        let cancel_flag = cancel_flag.clone();
        move || {
            if let Some(cancel) = cancel_flag.borrow().as_ref() {
                cancel.store(true, Ordering::Relaxed);
            }
            slint::CloseRequestResponse::HideWindow
        }
    });

    window.run()
}

/// Abre o arquivo de log, se a caixa estiver marcada.
///
/// `Err` carrega a mensagem para a janela e significa "não inicie a
/// varredura": ou o usuário desistiu no diálogo, ou o arquivo não pôde ser
/// criado. Caixa desmarcada devolve um log desligado, que engole tudo.
fn open_log_if_requested(
    window: &AppWindow,
    roots: &[PathBuf],
    started_at: &str,
) -> Result<SweepLog, String> {
    if !window.get_log_enabled() {
        return Ok(SweepLog::disabled());
    }

    let Some(target) = rfd::FileDialog::new()
        .set_title("Onde gravar o log da varredura")
        .set_file_name(log::suggested_file_name(started_at))
        .add_filter("Arquivo de texto", &["txt"])
        .set_directory(dirs::document_dir().unwrap_or_else(std::env::temp_dir))
        .save_file()
    else {
        return Err("Varredura não iniciada: nenhum destino escolhido para o log.".into());
    };

    SweepLog::create(&target, roots, started_at)
        .map_err(|e| format!("Varredura não iniciada: não deu para criar o log ({e})."))
}

/// Insere os subdiretórios logo abaixo da linha, um nível mais fundo.
///
/// `set_vec` em vez de `insert` num loop: expandir uma pasta com milhares de
/// subpastas emitiria uma notificação de modelo por linha. Uma substituição
/// só é uma notificação só. A `ListView` é virtualizada, então o custo de
/// render não cresce com o tamanho da lista.
fn expand(rows: &Rows, index: usize, selection: &HashSet<PathBuf>) {
    let mut all: Vec<TreeRow> = rows.iter().collect();
    let Some(row) = all.get(index) else {
        return;
    };

    let children = tree::subdirectories(Path::new(row.path.as_str()), row.depth + 1);
    if children.is_empty() {
        // Sem subpastas (ou sem permissão de ler): a seta some em vez de
        // ficar prometendo conteúdo que nunca aparece.
        all[index].expandable = false;
        rows.set_vec(all);
        return;
    }

    let new_rows: Vec<TreeRow> = children
        .iter()
        .map(|node| TreeRow {
            path: node.path.display().to_string().into(),
            name: node.name.clone().into(),
            detail: SharedString::new(),
            size_text: SharedString::new(),
            depth: node.depth,
            expandable: !node.protected,
            expanded: false,
            // Lida da seleção, não do estado anterior da linha: é isto que
            // faz a marcação sobreviver a colapsar e reexpandir o pai.
            selected: selection.contains(&node.path),
            protected: node.protected,
            removable: false,
        })
        .collect();

    all.splice(index + 1..index + 1, new_rows);
    all[index].expanded = true;
    rows.set_vec(all);
}

/// Remove todas as linhas descendentes. A seleção delas continua viva no
/// `HashSet` e reaparece se o nó for expandido de novo.
fn collapse(rows: &Rows, index: usize) {
    let mut all: Vec<TreeRow> = rows.iter().collect();
    let depths: Vec<i32> = all.iter().map(|row| row.depth).collect();
    let count = tree::descendant_count(&depths, index);

    all.drain(index + 1..index + 1 + count);
    all[index].expanded = false;
    rows.set_vec(all);
}
