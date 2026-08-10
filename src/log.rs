use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::sweep::SweepStats;

/// Log gravado ao vivo, uma linha por pasta tocada.
///
/// Escreve direto no arquivo em vez de acumular em memória: varrer um disco
/// inteiro analisa centenas de milhares de pastas, e guardar todo caminho até
/// o fim faria o consumo crescer junto com a árvore. Como efeito colateral, o
/// que já foi analisado sobrevive a um fechamento no meio da corrida.
///
/// O preço é o formato: em stream não dá para agrupar "apagadas" e
/// "analisadas" em seções, porque as duas chegam intercaladas. Cada linha
/// carrega a própria tag e a ordem é cronológica.
pub struct SweepLog {
    /// `None` quando o log está desligado, ou quando uma escrita falhou e
    /// paramos de tentar.
    sink: Option<BufWriter<File>>,
    path: Option<PathBuf>,
    /// Primeira falha de escrita. Só a primeira interessa: as seguintes são
    /// consequência dela.
    failed: Option<String>,
}

/// O que aconteceu com o log, para virar mensagem na janela.
pub enum LogOutcome {
    Disabled,
    Written(PathBuf),
    Failed(String),
}

impl SweepLog {
    pub fn disabled() -> Self {
        Self {
            sink: None,
            path: None,
            failed: None,
        }
    }

    /// Cria o arquivo e escreve o cabeçalho.
    ///
    /// Falhar aqui é diferente de falhar no meio: o usuário ainda não iniciou
    /// nada, então o erro sobe e quem chama decide (a janela não começa a
    /// varredura).
    pub fn create(path: &Path, roots: &[PathBuf], now: &str) -> io::Result<Self> {
        let mut sink = BufWriter::new(File::create(path)?);

        // CRLF em todas as quebras: o destino natural deste arquivo é um
        // duplo-clique no Windows, e o Bloco de Notas antigo cola tudo numa
        // linha só com LF puro.
        write!(sink, "Folder Sweep — log de varredura\r\n")?;
        write!(sink, "Iniciado em: {now}\r\n")?;
        write!(sink, "Caminhos marcados:\r\n")?;
        for root in roots {
            write!(sink, "  {}\r\n", root.display())?;
        }
        write!(sink, "\r\n")?;
        sink.flush()?;

        Ok(Self {
            sink: Some(sink),
            path: Some(path.to_path_buf()),
            failed: None,
        })
    }

    pub fn record_scanned(&mut self, path: &Path) {
        self.line("ANALISADA", path, None);
    }

    pub fn record_deleted(&mut self, path: &Path) {
        self.line("APAGADA  ", path, None);
    }

    pub fn record_error(&mut self, path: &Path, message: &str) {
        self.line("ERRO     ", path, Some(message));
    }

    /// Empurra o buffer para o disco.
    ///
    /// Chamado no mesmo ritmo do progresso da UI. Sem isso, o que está no
    /// `BufWriter` só chegaria ao arquivo quando ele enchesse — e o log de
    /// uma corrida longa ficaria minutos atrás do que a janela mostra.
    pub fn flush(&mut self) {
        let Some(sink) = self.sink.as_mut() else {
            return;
        };
        if let Err(e) = sink.flush() {
            self.give_up(e);
        }
    }

    /// Escreve o rodapé e fecha. Consome o log: depois disto não há mais o que
    /// registrar.
    pub fn finish(
        mut self,
        stats: SweepStats,
        elapsed: Duration,
        cancelled: bool,
        now: &str,
    ) -> LogOutcome {
        if let Some(sink) = self.sink.as_mut() {
            let footer = (|| -> io::Result<()> {
                write!(sink, "\r\n")?;
                write!(
                    sink,
                    "{} em: {now}\r\n",
                    if cancelled { "Cancelado" } else { "Concluído" }
                )?;
                write!(sink, "Duração: {}\r\n", format_elapsed(elapsed))?;
                write!(
                    sink,
                    "Resumo: {} analisadas · {} apagadas · {} erros\r\n",
                    stats.scanned, stats.deleted, stats.errors
                )?;
                sink.flush()
            })();

            if let Err(e) = footer {
                self.give_up(e);
            }
        }

        match (self.failed, self.path) {
            (Some(message), _) => LogOutcome::Failed(message),
            (None, Some(path)) => LogOutcome::Written(path),
            (None, None) => LogOutcome::Disabled,
        }
    }

    fn line(&mut self, tag: &str, path: &Path, message: Option<&str>) {
        let Some(sink) = self.sink.as_mut() else {
            return;
        };

        let written = match message {
            Some(message) => write!(sink, "{tag}  {} — {message}\r\n", path.display()),
            None => write!(sink, "{tag}  {}\r\n", path.display()),
        };
        if let Err(e) = written {
            self.give_up(e);
        }
    }

    /// Larga o arquivo na primeira falha.
    ///
    /// Uma varredura não é abortada porque o log parou de escrever — apagar
    /// pastas vazias continua sendo o trabalho, e interromper no meio deixaria
    /// o disco num estado que o usuário não pediu. O erro é guardado e vira
    /// mensagem no fim.
    fn give_up(&mut self, error: io::Error) {
        self.sink = None;
        if self.failed.is_none() {
            self.failed = Some(error.to_string());
        }
    }
}

pub fn now_text() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Nome sugerido no diálogo de salvar, derivado do mesmo texto do cabeçalho.
/// Os segundos caem: não ajudam a distinguir duas corridas e só alongam o nome.
pub fn suggested_file_name(now: &str) -> String {
    let stamp = match now.split_once(' ') {
        Some((date, time)) => {
            let hhmm: Vec<&str> = time.split(':').take(2).collect();
            format!("{date}-{}", hhmm.join("-"))
        }
        None => now.replace(':', "-"),
    };
    format!("foldersweep-{stamp}.txt")
}

pub fn format_elapsed(elapsed: Duration) -> String {
    let total = elapsed.as_secs();
    let (hours, minutes, seconds) = (total / 3600, (total % 3600) / 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_file(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("foldersweep-log-{name}-{nanos}.txt"))
    }

    #[test]
    fn writes_header_entries_and_footer() {
        let file = temp_file("completo");
        let mut log = SweepLog::create(
            &file,
            &[PathBuf::from(r"D:\Projetos")],
            "2026-08-09 14:33:02",
        )
        .unwrap();

        log.record_scanned(Path::new(r"D:\Projetos\a"));
        log.record_deleted(Path::new(r"D:\Projetos\a"));
        log.record_error(Path::new(r"D:\Projetos\b"), "acesso negado");

        let stats = SweepStats {
            scanned: 1,
            deleted: 1,
            errors: 1,
        };
        let outcome = log.finish(stats, Duration::from_secs(7), false, "2026-08-09 14:36:09");
        assert!(matches!(outcome, LogOutcome::Written(_)));

        let content = std::fs::read_to_string(&file).unwrap();
        assert!(content.contains("Iniciado em: 2026-08-09 14:33:02"));
        assert!(content.contains(r"  D:\Projetos"));
        assert!(content.contains(r"ANALISADA  D:\Projetos\a"));
        assert!(content.contains(r"APAGADA    D:\Projetos\a"));
        assert!(content.contains(r"ERRO       D:\Projetos\b — acesso negado"));
        assert!(content.contains("Concluído em: 2026-08-09 14:36:09"));
        assert!(content.contains("Duração: 0:07"));
        assert!(content.contains("Resumo: 1 analisadas · 1 apagadas · 1 erros"));

        std::fs::remove_file(&file).ok();
    }

    #[test]
    fn entries_reach_the_disk_before_the_run_ends() {
        let file = temp_file("streaming");
        let mut log =
            SweepLog::create(&file, &[PathBuf::from(r"D:\")], "2026-08-09 14:33:02").unwrap();

        log.record_deleted(Path::new(r"D:\vazia"));
        log.flush();

        // Sem `finish`: é exatamente o estado de uma varredura em andamento.
        let content = std::fs::read_to_string(&file).unwrap();
        assert!(content.contains(r"APAGADA    D:\vazia"));

        std::fs::remove_file(&file).ok();
    }

    #[test]
    fn cancelled_run_says_so_in_the_footer() {
        let file = temp_file("cancelado");
        let log = SweepLog::create(&file, &[], "2026-08-09 14:33:02").unwrap();
        log.finish(
            SweepStats::default(),
            Duration::ZERO,
            true,
            "2026-08-09 14:33:09",
        );

        let content = std::fs::read_to_string(&file).unwrap();
        assert!(content.contains("Cancelado em: 2026-08-09 14:33:09"));

        std::fs::remove_file(&file).ok();
    }

    #[test]
    fn disabled_log_swallows_everything() {
        let mut log = SweepLog::disabled();
        log.record_scanned(Path::new(r"D:\a"));
        log.record_deleted(Path::new(r"D:\a"));
        log.flush();

        let outcome = log.finish(SweepStats::default(), Duration::ZERO, false, "agora");
        assert!(matches!(outcome, LogOutcome::Disabled));
    }

    #[test]
    fn suggests_a_name_without_seconds() {
        assert_eq!(
            suggested_file_name("2026-08-09 14:33:02"),
            "foldersweep-2026-08-09-14-33.txt"
        );
    }

    #[test]
    fn formats_elapsed_under_and_over_an_hour() {
        assert_eq!(format_elapsed(Duration::from_secs(0)), "0:00");
        assert_eq!(format_elapsed(Duration::from_secs(7)), "0:07");
        assert_eq!(format_elapsed(Duration::from_secs(187)), "3:07");
        assert_eq!(format_elapsed(Duration::from_secs(3723)), "1:02:03");
    }
}
