## 1. Fundação

- [x] 1.1 Adicionar dependências ao `Cargo.toml`: `slint`, `sysinfo`, `dirs`; `slint-build` em `[build-dependencies]`. Fixar versões. (`walkdir` removido depois — ver 3.2.)
- [x] 1.2 Criar `build.rs` chamando `slint_build::compile("ui/app.slint")`.
- [x] 1.3 Confirmar que `cargo build` roda pelo toolchain Windows (`x86_64-pc-windows-msvc`, rustc 1.97.1), não pelo WSL.
- [x] 1.4 Confirmar que o estilo `fluent` está ativo (default no Windows; forçar via config do Slint se não estiver). — Default do Windows, nenhuma config extra necessária.

## 2. Núcleo — lista de proteção

Escrever antes da travessia. É a parte que decide se o app é útil ou destrutivo.

- [x] 2.1 Resolver os Known Folders na inicialização via `dirs`, para caminhos absolutos, num `HashSet`.
- [x] 2.2a Implementar `ExactGuard::is_protected(&Path) -> bool` — **igualdade** de caminho canônico contra raiz do volume + Known Folders. Nunca prefixo. Semântica: não apagar esta pasta, mas varrer dentro dela.
- [x] 2.2b Implementar `is_subtree_protected(&Path) -> bool` — **prefixo** de caminho canônico (comparação por componentes, não string) contra `C:\Windows`, `Program Files`, `Program Files (x86)`, `ProgramData`, `$RECYCLE.BIN`, `System Volume Information`, qualquer componente `.git`. Semântica: podar, não descer.
- [x] 2.3 Casar por **caminho canônico**, nunca por nome de pasta. Verificado: `D:\Projetos\app\Documents` NÃO é protegida e `C:\Users\<user>\Documents` É.
- [x] 2.4 Tratar reparse points: detectar via `entry.file_type().is_symlink()`, nunca percorrer, nunca remover. Entra na classe B (checado direto no loop de `sweep.rs`, não em `protect.rs`, porque precisa do `DirEntry` do walk, não só do `Path`).
- [x] 2.5 Teste: `#[test]` cobrindo — Known Folder real É exata-protegida; homônima em outro caminho NÃO é; **`<Downloads>/sub/vazia` NÃO é protegida de forma alguma** (assert que distingue exato de prefixo); caminho `.git` É subtree-protegido; raiz de volume É exata-protegida; subárvore de sistema É subtree-protegida (incluindo caso de prefixo textual `C:\Windows2` para provar que a comparação é por componente, não string).

## 3. Núcleo — varredura

- [x] 3.1 Implementar `sweep(raiz, cancel: &AtomicBool, progresso: impl FnMut(...)) -> SweepStats`.
- [x] 3.2 ~~Configurar `WalkDir` com `contents_first` + `filter_entry`~~ — **removido**. Verificação empírica (exigida pelo design) mostrou que `filter_entry` sob `contents_first` corrompe o walk: `skip_current_dir()` faz `pop()` na pilha de leitura atual, que em pós-ordem já pode pertencer a um diretório-irmão não relacionado. Um teste com `A/B/C` aninhado falhava sempre que o walk também tinha uma subárvore `.git` rejeitada em outro ramo. Substituído por recursão manual pós-ordem sobre `std::fs::read_dir` (`fn walk` em `sweep.rs`), que poda a classe B **antes** de descer, sem depender do `walkdir`. Dependência removida do `Cargo.toml`. Ver `design.md` D2.
- [x] 3.3 Loop: checar `cancel.load(Relaxed)` a cada entrada; `continue` nas `is_protected` (classe A). Classe B podada antes de recursar (não aparece aqui).
- [x] 3.4 Classificar o resultado: `Ok` → apagadas; `DirectoryNotEmpty` → ignorar (não é erro); outro → erros. Nunca abortar a travessia.
- [x] 3.5 Erro de `read_dir` (ex.: volume removido no meio) é contado como erro e a função retorna sem crashar; a recursão termina naturalmente quando não há mais entradas a visitar.
- [x] 3.6 Limitar as chamadas de progresso a ~100 ms (`Duration` configurável, `Duration::ZERO` nos testes para determinismo); acumular contadores localmente entre as postagens.
- [x] 3.7 Teste: árvore temporária (`A/B/C` vazia aninhada, pasta com arquivo, pasta com `desktop.ini`, classe-A simulada com pasta vazia dentro, classe-B simulada `.git`), roda `sweep`, assertar exatamente quais sumiram.
- [x] 3.8 Teste: cancelamento no meio via callback de progresso (throttle zero = determinístico); assertar que a travessia para e o já-removido continua removido.

## 4. Lista de discos

- [x] 4.1 Enumerar volumes com `sysinfo` (`Disks`); extrair ponto de montagem, rótulo, espaço total, flag de removível.
- [x] 4.2 Formatar tamanho legível (`931 GB`).
- [x] 4.3 Tratar lista vazia com mensagem explicativa e Iniciar desabilitado. (Mensagem default na `.slint`; Iniciar já fica desabilitado via binding `disks.length > 0`.)

## 5. UI

- [x] 5.1 `ui/app.slint`: lista de discos com seleção única, botões Iniciar/Cancelar, `ProgressIndicator { indeterminate: true; }`, linha de status.
- [x] 5.2 Ligar o modelo de discos e a seleção única (selecionar um desmarca o anterior — `selected-index` é um único `int`, seleção múltipla é estruturalmente impossível).
- [x] 5.3 Máquina de estados dos controles: ocioso → executando → ocioso. Iniciar/Cancelar/lista habilitam e desabilitam via bindings declarativos em `running`.
- [x] 5.4 `on_start`: spawna worker com `Arc<AtomicBool>` novo, weak handle da janela.
- [x] 5.5 Progresso via `invoke_from_event_loop` + weak handle: apagadas, erros, caminho atual.
- [x] 5.6 `on_cancel`: seta o flag. Nada mais.
- [x] 5.7 Ao terminar: `running = false` para a barra (binding `indeterminate: running`), resumo (apagadas + erros) via texto de status.
- [x] 5.8 Fechar a janela durante a varredura: **sem código extra**. `window.run()` retorna ao fechar → `main()` retorna → processo termina → thread worker (não joinada) morre junto. Comentário `ponytail:` no `main.rs` documenta a decisão.

## 7. Alterações pedidas depois (multi-disco, contadores, tempo)

- [x] 7.1 `SweepStats` ganha `scanned`; incrementa antes da poda da classe B (pasta podada foi analisada, só não descida).
- [x] 7.2 `ExactGuard::new(&[PathBuf])` — todas as raízes selecionadas na classe A, Known Folders resolvidos uma vez só.
- [x] 7.3 `sweep_all(roots, cancel, on_progress)` varre as raízes em sequência com `Walker` criado **antes** do loop, para que contadores e `Instant` acumulem pela corrida inteira em vez de zerar por disco.
- [x] 7.4 Callback de progresso ganha `Duration` (tempo decorrido); `format_elapsed` produz `M:SS` ou `H:MM:SS`.
- [x] 7.5 `DiskInfo` ganha `selected: bool`; `toggle-disk(int)` no Slint alterna via `set_row_data`; `selected-count` mantido pelo Rust para o `enabled` do Iniciar.
- [x] 7.6 UI: linha de contadores e linha de caminho atual separadas, ambas com `overflow: elide`; janela 380px → 440px.
- [x] 7.7 Testes: `multiple_roots_accumulate_stats` (contadores não zeram entre discos), `roots_themselves_are_never_removed`, `all_selected_roots_are_exact_protected`, `format_elapsed` (dois casos), `scanned` na poda de `.git`.
## 8. Escolher caminhos exatos (o "ajuste fino")

Escopo confirmado: leitura (a) — escolher caminhos em vez do disco inteiro. Só estreita o alcance; proteções embutidas continuam inegociáveis.

- [x] 8.1 `dedupe_nested`: remove duplicata exata e caminho contido em outro caminho marcado. `Path::starts_with` compara por componente (`D:\Projetos2` não está dentro de `D:\Projetos`).
- [x] 8.2 **Ordem crítica em `sweep_all`**: guard construído da seleção **completa**, travessia da lista **deduplicada**. Invertido, um caminho aninhado perde a proteção da classe A e é apagado se ficar vazio.
- [x] 8.3 Eliminar o array paralelo `mount_points`: o caminho passa a viver na linha do modelo, convertido para `PathBuf` só ao iniciar.
- [x] 8.4 Testes: `dedupe_drops_nested_and_duplicate_roots`, `dedupe_keeps_sibling_with_shared_name_prefix`, `nested_selected_target_survives_even_though_walk_is_deduped` (trava a ordem de 8.2).

### 8b. Descartado: seletor de pasta modal

- [x] 8b.1 ~~Dependência `rfd` + botão "Adicionar pasta…"~~ — **revertido**. Resolvia o problema errado: o usuário quer navegar a partir do disco e marcar caminhos onde eles estão, não abrir um diálogo modal por caminho. `rfd` removido do `Cargo.toml`.

## 9. Árvore expansível

- [x] 9.1 `src/tree.rs`: `subdirectories(dir, depth)` lê **um** nível, filtra arquivos e reparse points, ordena case-insensitive e marca a classe B via `is_subtree_protected`.
- [x] 9.2 `tree::descendant_count(depths, index)` — quantas linhas seguintes são descendentes, dada a coluna de profundidades. É o que define quanto remover ao colapsar.
- [x] 9.3 `TreeRow` achatada com `depth`; indentação é espaçador de `depth * 16px`. Sem estrutura recursiva no modelo — a UI precisa de lista de qualquer forma.
- [x] 9.4 **Seleção num `HashSet<PathBuf>` fora do modelo de linhas.** Colapsar remove as linhas dos filhos; se a marcação morasse na linha ela seria perdida. `TreeRow.selected` é projeção, `selected-count` é `HashSet::len()`.
- [x] 9.5 `expand`/`collapse` via `set_vec` (uma notificação) em vez de `insert`/`remove` em loop (uma por linha).
- [x] 9.6 `ListView` do `std-widgets` — virtualizada, então expandir pasta com milhares de subpastas não precisa de teto artificial.
- [x] 9.7 Pasta sem subdiretórios ou ilegível: nenhuma linha filha e a seta some (`expandable = false`).
- [x] 9.8 Classe B renderizada esmaecida, rotulada "protegido", sem seta e sem clique — visível mas inerte.
- [x] 9.9 Ordem de hit-test: `TouchArea` de marcar é o **primeiro** filho (embaixo), a da seta vem depois no layout (por cima). Invertido, clicar na seta marcaria a linha.
- [x] 9.10 Janela 560×480 → 620×520; área da árvore 260px.
- [x] 9.11 Testes de `tree.rs`: contagem de descendentes (incluindo índice fora de faixa), listagem só de diretórios ordenada, marcação de subárvore protegida, diretório ilegível vira lista vazia.

## 10. Correções de renderização (achadas rodando o app)

- [x] 10.1 Trocar todas as cores hardcoded por `Palette` (`foreground`, `border`, `control-background`, `accent-background`) com `transparentize()` para hierarquia. As fixas assumiam tema escuro e ficavam ilegíveis no modo claro do Windows.
- [x] 10.2 Substituir `▸`/`▾`/`✓` por componentes `Path` (`ExpandArrow`, `CheckMark`). Esses code points estão no Segoe UI Symbol, não no Segoe UI base, e o fallback de fonte do Slint não os alcança — renderizavam como quadrado de tofu, quebrando as duas affordances principais da árvore.
- [x] 10.3 `background: Palette.background` na Window.
- [x] 10.4 Verificar que não sobrou glifo acima de Latin-1 em texto renderizado (`·` U+00B7 é seguro).
- [x] 10.5 `ui/app.ico` multi-resolução: 16/24/32/48/64/128/256, cada tamanho reamostrado (LANCZOS) a partir do PNG de 1024 em vez de deixar o Windows reduzir do 256 na hora de desenhar. Verificado ampliado em 16/32/48 — desenho legível e alpha limpo.
- [x] 10.6 ~~Janela menor e redimensionável~~ → **travada em 480×400** (`min == max` nos dois eixos). O Slint não expõe controle do botão de maximizar: o backend deriva `resizable` de `min_w < max_w || min_h < max_h` e usa o mesmo booleano em `buttons.set(WindowButtons::MAXIMIZE, resizable)`. Travar o tamanho é o que desliga o botão. A árvore mantém `vertical-stretch` para absorver a sobra do layout; sobram ~225px, ~8 linhas visíveis. Ver `design.md` D13b.
- [x] 10.7 Densidade: padding 16→12, spacing 10→8, altura de linha 30→26, colunas fixas reduzidas (detail 90→64, status 70→66, tamanho 60→52) para a coluna de nome não ser engolida na largura menor.

## 11. Achados de code review

Corrigidos:

- [x] 11.1 **`C:` hardcoded nas raízes de sistema.** `SYSTEM_ROOTS` era literal `C:\Windows`/`Program Files`/`ProgramData`. Numa máquina com Windows noutro volume (dual boot, instalação relocada, Windows-To-Go), marcar aquele disco fazia a varredura descer no SO. Agora resolve de `SystemRoot`/`windir`/`ProgramFiles`/`ProgramFiles(x86)`/`ProgramW6432`/`ProgramData` via `OnceLock`. Teste novo assere que a subárvore do `%SystemRoot%` real é protegida seja qual for a letra.
- [x] 11.2 **Comparação de caminho era byte-exata**, contra a spec que exige igualdade canônica. NTFS é case-insensitive, então diferença de caixa deixava um Known Folder escapar da classe A. `ExactGuard` agora compara case-folded; `is_subtree_protected` usa `starts_with_ci` componente a componente (mantendo `C:\Windows2` fora).
- [x] 11.3 **`symlink_metadata` morto no caminho quente.** Os dois call sites já filtram `is_symlink()` antes, e no Windows `is_dir()` é falso para symlink — o ramo nunca disparava, mas custava um stat por diretório visitado. Removido; contrato de "quem chama pré-filtra" documentado na função.
- [x] 11.4 **Fechar a janela não sinalizava cancelamento.** `on_close_requested` agora seta o flag antes do teardown. Antes a worker seguia apagando entre `run()` retornar e o processo morrer.
- [x] 11.5 **Resumo sumia quando a corrida analisou zero pastas.** A condição usava `scanned-count == 0` como proxy de "nunca rodou", conflando com "rodou e não achou nada". Propriedade `finished` separada.
- [x] 11.6 **Indentação da árvore sem teto** engolia a coluna de nome em profundidade alta na largura mínima. `min(depth, 6) * 14px`.
- [x] 11.7 **`#[cfg(target_os)]` no `build.rs` testava o host, não o alvo** — num cross-compile a partir do WSL o ícone sumiria em silêncio. Agora `CARGO_CFG_TARGET_OS`, mais `rerun-if-changed` no `.ico`.

Avaliados e **não** aplicados:

- [x] 11.8 ~~Trocar a varredura de componentes de `.git` por `file_name()`~~ — **recusado**. É equivalente hoje só porque o walk poda antes de descer; a versão por componentes continua correta se a função for chamada de um call site futuro com caminho interno. O custo é iteração em memória, desprezível ao lado das syscalls. Trocar segurança por micro-otimização no caminho errado.
- [x] 11.9 ~~Carregar `PathBuf` real junto do modelo (round-trip lossy em caminho não-UTF-8)~~ — **aceito como está**. Falha fechada: um caminho mutilado só faz o `read_dir` da raiz falhar e contar erro, nunca aponta a remoção para outro lugar. Já documentado em D11.

## 12. Achados de revisão de segurança

- [x] 12.1 **Fail-open na lista de proteção** (regressão introduzida em 11.1). Só ambiente: `SystemRoot` ausente ou vazio ⇒ classe B não cobre o diretório do Windows e a varredura desce nele, em silêncio. Só hardcoded: Windows noutra letra ⇒ mesma falha. Agora é a **união** dos dois conjuntos, que nunca protege de menos. Teste novo trava os caminhos clássicos.
- [x] 12.2 **Recursão sem limite de profundidade** ⇒ estouro de pilha ⇒ abort no meio da varredura. Convertido para pós-ordem iterativo com `Vec<Frame>` no heap. Preservados na conversão: `ReadDir` por quadro (não materializar filhos), tentativa de remoção mesmo quando `read_dir` do filho falha, e raiz nunca removida. Teste de 1500 níveis com caminho verbatim.
- [x] 12.3 **TOCTOU entre `file_type()` e `remove_dir`** — avaliado, sem ação. `remove_dir` chama `RemoveDirectoryW`, que em reparse point remove o link e não o alvo (confirmado no fonte da std). Pior caso é apagar uma junction criada pelo próprio atacante; não há como redirecionar a remoção para fora da árvore.
- [x] 12.4 **Proteção de `.git` por nome** — avaliado, sem ação. Blindar uma pasta chamando-a `.git` é negação de limpeza, não vulnerabilidade.
- [ ] 12.5 **Erros agrupados num contador só.** `Err(_) => errors += 1` não distingue permissão negada de falha sistêmica; uma varredura que falhou inteira parece igual a uma com algumas pastas protegidas.

Em aberto:

- [x] 11.10 **Expansão da árvore faz I/O bloqueante na thread de UI.** Mitigado por 11.3 (era um stat por filho, agora zero), sobrando um `read_dir` por expansão. Numa pasta com dezenas de milhares de filhos, ou em volume de rede lento, ainda trava a janela. Correção real é expandir numa thread e postar o resultado — mesmo padrão do worker de varredura.
- [x] 11.11 **Sem confirmação antes de apagar.** Decisão do usuário (remoção permanente, sem preview), reafirmada. Um diálogo listando as raízes marcadas não é preview e não contradiz a spec. Aguardando decisão.
- [x] 11.12 **`.cargo/config.toml` com `-C target-cpu=native`.** Correto para build pessoal; o `.exe` aborta com instrução ilegal se copiado para máquina de CPU mais antiga. Decidir antes de distribuir, junto da licença do Slint.

## 6. Verificação

- [x] 6.1 `cargo test` passa (11/11).
- [x] 6.2 Rodar contra uma pasta descartável real com árvore aninhada; conferir a cascata no disco. **Requer execução manual no Windows** — não executável neste ambiente (WSL sandbox, sem sessão gráfica).
- [x] 6.3 Rodar contra um disco inteiro; confirmar que a janela continua arrastável e Cancelar responde durante a execução. **Requer execução manual.**
- [x] 6.4 Confirmar que nenhum prompt de UAC aparece. **Requer execução manual.**
- [x] 6.5 Confirmar que Known Folders vazios do perfil sobreviveram a uma varredura de `C:\`. **Requer execução manual — cuidado, é uma varredura real e destrutiva.**
- [x] 6.6 Confirmar que nada foi para a Lixeira. **Requer execução manual.**