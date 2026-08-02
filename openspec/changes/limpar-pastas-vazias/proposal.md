## Why

Discos acumulam pastas vazias deixadas por desinstaladores, extrações de zip, builds e movimentações de arquivo. Nenhuma ferramenta nativa do Windows remove isso, e fazer à mão é inviável porque a vacuidade é recursiva: uma pasta que só contém pastas vazias também está vazia depois que os filhos somem.

O `foldersweep` é um app desktop Windows de janela única que varre um disco e apaga essas pastas. O escopo é deliberadamente minúsculo: lista de discos, iniciar, cancelar, barra de progresso.

## What Changes

- Novo app desktop Windows em Rust, GUI com **Slint** no estilo `fluent` (aparência nativa moderna).
- **Árvore expansível** de caminhos: discos montados como raízes, expandindo sob demanda um nível por vez. O usuário marca **um ou mais** caminhos em qualquer nível e em qualquer combinação de discos, varridos em sequência numa única corrida.
- Botão **Iniciar** dispara a varredura numa thread separada; a UI nunca bloqueia.
- Botão **Cancelar** para a varredura de forma cooperativa via `AtomicBool`.
- **Barra de progresso indeterminada** (vai e volta) — sinaliza "não travei", não percentual. Nenhuma fase de contagem prévia é executada.
- Contadores ao vivo acumulados pela corrida inteira: pastas analisadas, apagadas, erros e tempo decorrido — numa linha; a pasta sendo verificada agora numa linha separada abaixo.
- Remoção é **permanente** via `std::fs::remove_dir` (sem Lixeira, sem quarentena, sem undo).
- Varredura **bottom-up** (`walkdir` com `contents_first`), o que faz a cascata de pastas aninhadas acontecer numa passada só.
- Lista de caminhos protegidos que nunca são apagados nem percorridos.

## Capabilities

### New Capabilities

- `disk-selection`: enumerar os discos montados, apresentá-los como árvore expansível carregada sob demanda, e permitir marcar um ou mais caminhos como raízes da varredura.
- `empty-folder-sweep`: percorrer a raiz escolhida de baixo pra cima, apagar pastas vazias permanentemente, respeitar a lista de proteção, reportar progresso e obedecer cancelamento.

### Modified Capabilities

Nenhuma. Projeto novo, `openspec/specs/` está vazio.

## Impact

**Código**: projeto é uma folha em branco — `src/main.rs` tem 3 linhas, zero dependências, zero commits.

**Dependências** (4):

| Crate | Para quê | Por que não fazer à mão |
|---|---|---|
| `slint` | GUI, estilo fluent | escrever árvore WinUI/Win32 à mão é ordem de grandeza mais código |
| `sysinfo` | listar discos | evita FFI de `GetLogicalDrives`/`GetDriveTypeW` |
| `dirs` | resolver Known Folders | resolve caminho real via `SHGetKnownFolderPath` sem puxar o crate `windows` inteiro |
| `build.rs` + `slint-build` | compilar `.slint` | exigido pelo Slint |

**`walkdir` foi removido durante a implementação.** A combinação `contents_first(true)` + `filter_entry` do walkdir 2.5 corrompe a travessia — confirmado empiricamente e no próprio código-fonte da crate (`skip_current_dir` faz `pop()` na pilha de leitura atual, que em modo pós-ordem já pode pertencer a um diretório-irmão não relacionado ao rejeitado). Como o app nunca segue symlink/junction, a principal vantagem do `walkdir` (evitar loop ao seguir links) não se aplicava. A travessia agora é uma recursão manual pós-ordem sobre `std::fs::read_dir` (~40 linhas), sem dependência. Ver `design.md` D2.

Cancelamento usa `AtomicBool` da std. Nenhuma dependência para isso.

**Build**: precisa do toolchain Windows (`x86_64-pc-windows-msvc`, rustc 1.97.1 instalado em `C:\Users\Matheus\.cargo`). Compilar pelo WSL não produz o executável certo. `edition = "2024"` já está no `Cargo.toml`.

**Licença**: Slint é GPLv3 / royalty-free / comercial paga. A royalty-free cobre app desktop, mas as condições precisam ser lidas se o `foldersweep` virar produto distribuído. Decisão pendente, não bloqueia o desenvolvimento.

## Decisões já tomadas (não reabrir)

1. **Permanente, não Lixeira.** Consequência importante: `remove_dir` falha com `ERROR_DIR_NOT_EMPTY` se a pasta não estiver vazia — garantia do kernel. Isso torna **impossível** perder arquivo, mesmo com corrida (arquivo criado entre a checagem e a remoção). O único dano possível é apagar uma pasta vazia que algo esperava existir; é contra isso, e só isso, que a lista de proteção existe.
2. **Vacuidade estrita.** Uma pasta contendo apenas `desktop.ini` ou `Thumbs.db` **não** está vazia. Sai de graça: o kernel recusa a remoção.
3. **Disco inteiro**, não só o perfil do usuário.
4. **Sem tela de preview.** Justificado por (1) — sem risco de perda de arquivo, preview não paga o custo.

## Suposição a confirmar

A pergunta "disco inteiro ou só o perfil?" foi respondida (*disco inteiro*); a pergunta emparelhada sobre excluir diretórios de sistema não foi. **Assumindo exclusão por padrão** de `C:\Windows`, `C:\Program Files`, `C:\Program Files (x86)` e `C:\ProgramData` — pastas vazias ali existem de propósito (servicing, dirs que instaladores esperam) e apagá-las quebra software sem liberar espaço relevante. Detalhado em `specs/empty-folder-sweep/spec.md`. Se a intenção era varrer inclusive esses caminhos, é só dizer e a regra sai.
