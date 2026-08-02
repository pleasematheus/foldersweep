## Context

App desktop Windows de janela única. Três widgets: lista de discos, dois botões, uma barra. O núcleo lógico real são ~50 linhas; o volume de decisão está em *quais pastas não tocar*, não em como percorrer.

Restrições fixadas antes do design:

- Rust puro, uma linguagem, um build.
- Aparência de app Windows moderno.
- Barra indeterminada (vai e volta), não percentual.
- Remoção permanente, sem undo.
- Vacuidade estrita (`desktop.ini` sozinho = não vazio).
- Disco inteiro.

## Goals / Non-Goals

**Goals**

- UI que não trava nunca, nem em varredura de disco cheio.
- Impossibilidade estrutural de perder arquivo.
- Cancelamento que responde em milissegundos.
- Diff mínimo: dependências poucas e óbvias.

**Non-Goals**

- Preview / dry-run / árvore de resultados.
- Undo, quarentena, integração com Lixeira.
- Múltiplos discos **em paralelo** (múltiplos discos em *sequência* passou a ser suportado — ver D8).
- Filtros, regras do usuário, exclusões configuráveis, perfis salvos.
- Suporte multiplataforma. Windows apenas.
- Elevação de privilégio.

## Decisions

### D1 — Slint, estilo `fluent`

**Escolhido:** `slint` com o estilo `fluent` (default no Windows).

**Alternativas descartadas:**

| Opção | Por que não |
|---|---|
| WinUI 3 via `windows-rs` | Não existe compilador de XAML para Rust. Montar a árvore de controles em código é verboso, sem designer, sem hot reload, sem precedente. |
| WinUI 3 em C# + core Rust via FFI | Duas linguagens, dois sistemas de build, P/Invoke — para três widgets. |
| C# puro | Funcionaria, mas descarta o Rust. |
| `egui` | Menos código, mas parece painel de debug de jogo. Falha no requisito de aparência. |
| `iced` | Sem visual nativo; barra indeterminada não é nativa. |
| Tauri / WebView2 | Toolchain de front-end inteiro para três widgets. |

Ganho concreto: `ProgressIndicator { indeterminate: true; }` já vem no estilo fluent. A barra pedida existe sem escrever animação nenhuma.

**Pendência:** licenciamento (GPLv3 / royalty-free / paga). Não bloqueia desenvolvimento; bloqueia distribuição.

### D2 — Recursão manual pós-ordem (não `walkdir`)

Vacuidade é recursiva. Travessia top-down exige repetir a passada até nada mais mudar:

```
top-down (ingênuo)              bottom-up (pós-ordem)
passa 1: nada vazio             passa 1: C vazia → apaga
passa 2: C vazia → apaga                 B vazia → apaga
passa 3: B vazia → apaga                 A vazia → apaga
passa 4: A vazia → apaga        fim.
... até estabilizar
```

Pós-ordem faz a cascata sair de graça. Zero código de loop de convergência.

**Tentativa original: `walkdir` com `.contents_first(true)` + `.filter_entry(...)` para podar a classe B.** Descartada — quebrada. A própria doc do walkdir avisa: com `contents_first`, `filter_entry` "is no different than calling the standard `Iterator::filter`" porque entradas de diretório só são emitidas *depois* de descidas. Na prática é pior que "sem poda": quando o predicado rejeita um diretório, `FilterEntry::next` chama `self.it.skip_current_dir()`, que faz `pop()` incondicional na pilha de leitura — em modo pós-ordem, o topo da pilha nessa hora frequentemente já pertence a um diretório-irmão não relacionado ao rejeitado, e o `pop()` indevido corrompe a leitura desse irmão. Confirmado empiricamente: um teste com `A/B/C` aninhado (nada protegido) falhava sempre que o walk também continha uma subárvore `.git` rejeitada em outro ramo — `A` sobrevivia porque a leitura do diretório-pai foi cortada cedo.

**Escolhido:** recursão manual sobre `std::fs::read_dir`, pós-ordem:

```rust
fn walk(dir, guard, cancel, ..) {
    for entry in fs::read_dir(dir)? {
        let file_type = entry.file_type()?;
        if !file_type.is_dir() || file_type.is_symlink() { continue; }  // arquivo/reparse point
        if is_subtree_protected(&path) { continue; }                    // classe B: poda antes de descer

        walk(&path, guard, cancel, ..);                                 // desce primeiro (pós-ordem)

        if !guard.is_protected(&path) {
            remove_dir(&path);                                          // classe A: nunca remove ela mesma
        }
    }
}
```

Ganhos sobre a tentativa com `walkdir`: poda de verdade antes de descer (não apenas filtra depois), sem dependência externa, sem bug de pilha corrompida. `file_type.is_symlink()` cobre junction do Windows — no Windows uma junction tem `FILE_ATTRIBUTE_DIRECTORY` **e** `FILE_ATTRIBUTE_REPARSE_POINT` simultaneamente, então checar só `is_symlink()` (sem depender de `is_dir()` ser falso) é o que evita seguir.

A raiz do volume nunca entra na recursão como candidata a remoção — `walk()` só chama `remove_dir` sobre as *entradas* que lê, nunca sobre o `dir` recebido como parâmetro. Raiz nunca é passada para `remove_dir`, sem precisar de `min_depth` equivalente.

### D3 — `remove_dir` é o oráculo de vacuidade

Não existe checagem prévia de vacuidade. Tenta remover; interpreta o erro.

```rust
match fs::remove_dir(path) {
    Ok(()) => apagadas += 1,
    Err(e) if e.kind() == ErrorKind::DirectoryNotEmpty => {}  // normal, não é erro
    Err(_) => erros += 1,                                      // negado/IO, segue
}
```

Três coisas caem juntas:

1. **Uma syscall por pasta** em vez de `read_dir` + `remove_dir`.
2. **Corrida impossível.** O kernel decide vacuidade no instante da remoção. Não há janela entre checar e apagar. Arquivo criado um microssegundo antes não é perdido.
3. **Vacuidade estrita sai de graça.** `desktop.ini` presente → kernel recusa. A regra do usuário vira comportamento sem uma linha de código.

`ErrorKind::DirectoryNotEmpty` verificado compilando no toolchain do projeto (WSL 1.96.0, Windows 1.97.1). Se algum dia faltar, o fallback é `raw_os_error() == Some(145)` (`ERROR_DIR_NOT_EMPTY`).

### D4 — Duas classes de proteção, dois mecanismos

Casar por **nome** é errado: `D:\Projetos\app\Documents` é lixo legítimo; `C:\Users\<user>\Documents` não é. Os Known Folders são resolvidos uma vez na inicialização, para caminho absoluto, via `dirs` (`SHGetKnownFolderPath` sem puxar o crate `windows` inteiro).

Mas resolver o caminho certo não basta — falta decidir **como comparar**. Prefixo único para tudo é um bug que mata o app:

```
dirs::home_dir()  → C:\Users\Matheus
prefixo           → todo o perfil protegido → varrer C: não apaga nada

Downloads         → C:\Users\Matheus\Downloads
prefixo           → C:\...\Downloads\zip-extraido\vazia protegida
                    justo onde pasta vazia mais se acumula
```

São duas classes:

| Classe | Membros | Semântica | Mecanismo |
|---|---|---|---|
| **A — caminho exato** | raiz do volume, Known Folders (Desktop, Documents, Downloads, Music, Videos, Pictures, home, AppData…) | não apagar *esta* pasta, mas **varrer dentro dela** | `==` de caminho canônico → `continue` |
| **B — subárvore** | `C:\Windows`, `Program Files`, `Program Files (x86)`, `ProgramData`, `$RECYCLE.BIN`, `System Volume Information`, `.git`, reparse points | não apagar nada aqui e nem descer | podar antes de descer |

Duas funções, não uma: `ExactGuard::is_protected` (classe A) e `is_subtree_protected` (classe B).

A classe B poda de verdade antes de descer — ver D2. A recursão manual checa `is_subtree_protected` **antes** de chamar `walk()` no filho, então `C:\Windows` e qualquer `.git` nunca são lidos.

A classe A NÃO pode entrar na checagem de poda — colocar Known Folders ali reintroduz exatamente o bug de cima (protegeria o conteúdo inteiro, não só a pasta).

`.git` é subárvore inteira, não caminhos específicos. Git mantém vários diretórios vazios de propósito; uma regra cobre a classe toda e não precisa acompanhar as versões do git.

### D5 — Cancelamento por `AtomicBool`

```
 thread UI (event loop Slint)
        │
        │ on_iniciar → thread::spawn
        ▼
 ┌──────────────── worker ─────────────────┐
 │ walk(raiz) — recursão manual pós-ordem  │
 │                                         │
 │ para cada entrada de read_dir(dir):     │
 │   if cancel.load(Relaxed)      { return}│◄── Arc<AtomicBool>
 │   if !is_dir || is_symlink  { continue }│◄── arquivo / junction
 │   if subtree_protegida(&path) {continue}│◄── classe B: poda, não desce
 │   walk(&path)  ← recursão antes         │
 │   if !protegida_exata(&path)            │◄── classe A: pula só ela
 │     remove_dir(&path) → contadores      │
 └─────────────────────────────────────────┘
        │
        │ throttle ~100ms
        ▼  invoke_from_event_loop → atualiza UI
```

`Relaxed` basta: o único dado compartilhado é o próprio flag, não há memória protegida por ele. Sem dependência — `AtomicBool` é std.

A unidade de trabalho é uma syscall atômica e independente. Isso apaga uma classe inteira de problemas: não existe "meio de uma remoção", não existe estado parcial, não existe rollback. Cancelar em 213 de 500 deixa 213 removidas e 287 no disco — estado consistente, sem reparo necessário.

### D6 — Atualizações de UI limitadas a ~100 ms

Varredura de disco inteiro gera milhões de entradas. Um `invoke_from_event_loop` por entrada inunda o event loop e a UI fica *menos* responsiva do que se não reportasse nada.

O worker acumula contadores localmente e só posta quando passaram ~100 ms desde a última postagem. Barra e números continuam parecendo contínuos para o olho humano.

### D7 — Sem elevação, deliberadamente

Rodar sem admin não é limitação — é a rede de proteção. Sem admin, o app **não consegue** apagar dentro de `C:\Windows` mesmo que a lista de proteção tenha um furo. Elevar transformaria um bug de filtro em dano ao sistema. Diretório inacessível é contado como erro e pulado.

### D8 — Múltiplos discos, em sequência, com estado acumulado

Seleção passou de única para múltipla. Três consequências, todas em como o estado é montado — não em como o walk funciona:

**1. Um guard para todas as raízes.** `ExactGuard::new` recebe `&[PathBuf]` em vez de `&Path`. Os Known Folders resolvem uma vez só (não uma vez por disco), e cada raiz selecionada entra no conjunto da classe A — então nenhuma raiz é removida, mesmo quando outra raiz é a que está sendo varrida.

**2. Contadores acumulam através da corrida, não por disco.** A armadilha aqui: `SweepStats::default()` dentro do loop zeraria a UI toda vez que um disco terminasse. O estado vive no `Walker`, criado uma vez antes do loop das raízes:

```
sweep_all(roots)
  ├── ExactGuard::new(roots)   ← uma vez, todas as raízes
  ├── started = Instant::now() ← uma vez, corrida inteira
  ├── Walker { stats: default, .. }
  └── for root in roots: walker.walk(root)   ← stats persiste entre raízes
```

**3. Cancelar mata a corrida, não o disco.** O `break` está no loop das raízes, então cancelar durante o disco 1 impede o disco 2 de começar.

Sequencial e não paralelo: a varredura é limitada por I/O e paralelizar exigiria `Mutex` nos contadores para nenhum ganho mensurável. `ponytail:` no código nomeia o teto e o caminho de upgrade.

### D9 — Contador de analisadas e tempo decorrido

`SweepStats` ganhou `scanned`. Incrementa quando o walk identifica uma entrada como diretório real — **antes** da poda da classe B, porque uma pasta podada foi de fato analisada (só não foi descida). Consequência visível: uma subárvore `.git` soma exatamente 1 em analisadas, não o tamanho dela.

Tempo decorrido viaja no mesmo callback de progresso que já existia, como um terceiro parâmetro. Sem `slint::Timer`, sem thread de relógio.

O teto disso é real e está marcado com `ponytail:` no código: o relógio só avança quando o walk emite progresso, então um `read_dir` lento numa pasta gigante congela o número por alguns segundos. A barra indeterminada continua animando nesse intervalo (ela é da thread de UI), então o sinal de "não travei" sobrevive — que era o trabalho dela desde o começo. Se o relógio parado incomodar, a troca é um `slint::Timer` de 500 ms na thread de UI.

### D10 — Duas linhas de status

Contadores e caminho atual competiam por uma linha só e o caminho era o que sumia. Agora:

```
┌────────────────────────────────────────────┐
│  ✓ C:   SSD        119 GB                  │
│  ✓ D:   SSD_3      931 GB                  │
│    E:   USB         57 GB   removível      │
├────────────────────────────────────────────┤
│  [ Iniciar ]  [ Cancelar ]                 │
│  ░░▓▓▓▓░░░░░░░░░░░                         │
│  12.483 analisadas · 213 apagadas · 0 erros · 2:07
│  D:\Projetos\algum\caminho\bem\longo…      │
└────────────────────────────────────────────┘
```

Ambas as linhas usam `overflow: elide` — caminho longo não estica a janela. Altura foi de 380px para 440px para caber a linha extra sem apertar a lista.

Seleção múltipla é marcada com `✓` mais realce de fundo. Não usei `CheckBox` do `std-widgets` de propósito: `checked` é `in-out` e o toggle vem do Rust via `set_row_data`, então haveria duas fontes de verdade disputando o mesmo estado. `TouchArea` + glifo tem uma fonte só — o modelo.

### D11 — Árvore expansível, carregada sob demanda

"Às vezes não quero limpar o disco inteiro, só um caminho específico." Primeira tentativa foi um botão "Adicionar pasta…" com seletor nativo (`rfd`) — **descartada**: resolve o problema errado. O usuário quer navegar a partir do disco e marcar caminhos onde eles estão, não abrir um diálogo modal e digitar/procurar um caminho de cada vez. `rfd` saiu das dependências.

A lista virou uma árvore. Discos são os nós raiz; expandir lê um nível de subdiretórios.

```
▾ D:\          SSD_3     931 GB
   ▸ Downloads
   ▾ Projetos
      ✓ app-antigo
      ▸ foldersweep
   ▸ Windows              protegido   ← esmaecido, inerte
▸ E:\          USB   57 GB  removível
```

**Achatada, não aninhada.** A árvore é um `Vec<TreeRow>` linear com um campo `depth`; a indentação é um espaçador de `depth * 16px`. Expandir insere as linhas filhas logo após o pai; colapsar remove as linhas seguintes enquanto `depth > depth do pai` (`tree::descendant_count`). Uma estrutura recursiva no modelo daria o mesmo resultado com muito mais código, porque a UI precisa de uma lista de qualquer forma.

**Um nível por vez.** Enumerar o disco inteiro para montar a árvore é exatamente o que a barra indeterminada existe para evitar (D3/D9). `tree::subdirectories` lê um `read_dir` e para. Erro de leitura vira lista vazia e a seta some — a árvore é navegação, não varredura, e não tem onde reportar erro.

**Seleção mora fora do modelo de linhas.** Este é o ponto que quebra se for feito do jeito óbvio:

```
seleção só na linha:
  marca D:\Projetos → colapsa D:\ → linha some → marcação perdida

seleção num HashSet<PathBuf>:
  marca D:\Projetos → colapsa D:\ → linha some → HashSet intacto
                    → reexpande   → linha renasce lendo o HashSet ✓
```

`TreeRow.selected` é projeção, não estado. `selected-count` é `HashSet::len()`.

**`set_vec` em vez de `insert`/`remove` em loop.** Expandir uma pasta com milhares de subpastas emitiria uma notificação de modelo por linha inserida. Uma substituição só é uma notificação só. A `ListView` do `std-widgets` é virtualizada, então o custo de render não cresce com o tamanho da lista — não precisa de teto artificial no número de filhos.

**Classe B aparece, esmaecida e inerte.** `C:\Windows`, `.git` e afins seriam podados na varredura de qualquer forma. Escondê-los faria o usuário procurar uma pasta que ele sabe que existe; mostrá-los marcáveis seria mentira. Aparecem rotulados "protegido", sem seta e sem clique.

**Ordem de hit-test no Slint.** A `TouchArea` de marcar cobre a linha inteira e é o **primeiro** filho; a `TouchArea` da seta vem depois, dentro do layout, portanto por cima. Invertido, clicar na seta também marcaria a linha.

Isso só **estreita** o alcance. Das três leituras de "ajuste fino" listadas nas Open Questions, esta é a (a). A (c) — afrouxar as proteções embutidas — continua fora, e a spec agora diz isso explicitamente.

**A ordem que decide a correção.** Guard e travessia saem de listas diferentes:

```
seleção do usuário: [ D:\ , D:\Projetos ]
                          │
        ┌─────────────────┴─────────────────┐
        ▼                                   ▼
  ExactGuard::new(TODAS)            dedupe_nested(TODAS)
  classe A = {D:\, D:\Projetos}     walk = [D:\]
        │                                   │
  protege as duas de remoção        percorre a subárvore uma vez só
```

Escrito ao contrário — guard a partir da lista deduplicada — `D:\Projetos` sai da classe A e some caso fique vazia durante a varredura de `D:\`. Uma pasta que o usuário nomeou de propósito, apagada porque ele também marcou o pai. As duas linhas ficam adjacentes em `sweep_all` com um comentário explicando por quê, e `nested_selected_target_survives_even_though_walk_is_deduped` trava o comportamento.

`dedupe_nested` remove duplicata exata primeiro, depois qualquer alvo contido em outro. `Path::starts_with` compara por componente, então `D:\Projetos2` não é considerado dentro de `D:\Projetos` — tem teste.

**Fim do array paralelo.** Antes, `mount_points: Rc<Vec<PathBuf>>` era indexado junto com o modelo do Slint. Com linhas entrando e saindo a cada expandir/colapsar, manter duas estruturas alinhadas por índice é criadouro de bug. O caminho vive na própria linha (`TreeRow.path`) e a varredura o converte de volta para `PathBuf` ao iniciar. Round-trip por `String` é lossy para caminho não-UTF-8, mas um caminho desses apareceria errado na árvore de qualquer forma — exibição e varredura permanecem consistentes entre si.

**Sem `alignment: start` no Slint.** Colide com o nome do callback `start` — o compilador reporta "Callback must be called. Did you forgot the '()'?", apontando para o lugar errado. Já mordeu duas vezes nesta base.

### D12 — Cores do tema e glifos vetoriais

Dois bugs achados rodando o app de verdade, ambos por assumir um ambiente ideal que a máquina real não entrega.

**Cores hardcoded assumiam tema escuro.** `#dddddd`, `#aaaaaa`, `#777777` — legíveis sobre fundo escuro, quase invisíveis sobre o branco do modo claro do Windows. O estilo `fluent` já segue o tema do sistema, mas texto com cor fixa não segue nada. Agora tudo sai de `Palette` (`foreground`, `border`, `control-background`, `accent-background`), com `transparentize()` para hierarquia visual em vez de tons cinza fixos. Funciona nos dois temas sem ramificação.

**Glifos Unicode viravam quadrado de tofu.** `▸` (U+25B8), `▾` (U+25BE) e `✓` (U+2713) não estão no Segoe UI base — moram no Segoe UI Symbol. O fallback de fonte do renderer do Slint não os alcança, então a seta de expandir e a marca de seleção apareciam como quadrado vazio. Isso quebrava justamente as duas affordances principais da árvore.

Trocados por `Path` com comandos SVG (`ExpandArrow`, `CheckMark`). Não dependem de fonte nenhuma, escalam sem serrilhar e aceitam `fill`/`stroke` do `Palette`. Mais linhas que um caractere, mas um caractere que não renderiza custa zero linha e a UI inteira junto.

Regra que fica: **nada de glifo Unicode acima de Latin-1 em texto renderizado.** `·` (U+00B7) é seguro; qualquer coisa nos blocos de símbolos não é. Se precisar de ícone, `Path`.

### D13 — Janela menor, e a árvore é quem cresce

Era 620×520 fixa, com a área da árvore travada em 260px. Duas coisas erradas nisso: ocupava tela demais para o que mostra, e aumentar a janela só produzia folga vazia — a árvore continuava do mesmo tamanho.

Agora `preferred` 480×400 e `min` 400×300, com a árvore em `vertical-stretch: 1` e `min-height: 120px`. Redimensionar dá mais linhas visíveis, que é a única dimensão que o usuário realmente quer ganhar.

As colunas fixas precisaram encolher junto (detail 90→64, status 70→66, tamanho 60→52). Na largura antiga sobrava espaço; em 400px elas engoliriam a coluna de nome, que é a que tem `horizontal-stretch` e carrega a informação. Altura de linha 30→26 e padding/spacing 16/10→12/8 pela mesma razão: em janela pequena, densidade é legibilidade.

### D14 — Raízes de sistema vêm do ambiente

`C:\Windows` hardcoded era o furo mais sério da lista de proteção, e passava despercebido porque a máquina de desenvolvimento tem o Windows em C:.

Windows não mora obrigatoriamente em C:. Dual boot, instalação relocada e Windows-To-Go colocam o SO noutra letra. Nessa máquina, marcar aquele disco na árvore fazia a classe B não casar com nada e a varredura descia no SO — apagando diretórios vazios que servicing e instaladores esperam encontrar.

Agora sai de `SystemRoot`, `windir`, `ProgramFiles`, `ProgramFiles(x86)`, `ProgramW6432` e `ProgramData`, resolvidos uma vez num `OnceLock`.

**Comparação case-insensitive.** A spec sempre pediu igualdade *canônica*; o código fazia igualdade de bytes. NTFS é case-insensitive, então `C:\Users\Matheus\Downloads` e `c:\users\matheus\downloads` são a mesma pasta, e um `HashSet<PathBuf>` erra o lookup entre as duas. Um erro aqui é um Known Folder saindo da classe A e sendo apagado.

`ExactGuard` agora guarda chaves case-folded. `is_subtree_protected` usa `starts_with_ci`, que compara **componente a componente** — folding a string inteira e usando `str::starts_with` faria `C:\Windows2` casar com `C:\Windows`.

Não coberto: nomes curtos 8.3 (`C:\Users\MATHEU~1`), `subst` e forma UNC vs. letra. `canonicalize()` resolveria, mas devolve caminhos com prefixo `\\?\` que quebrariam as comparações de prefixo, e custaria uma syscall por checagem no caminho quente. Fica anotado.

**Um stat por diretório a menos.** `is_subtree_protected` tinha um ramo de reparse point que nunca disparava: os dois call sites já filtram `is_symlink()` antes, e no Windows `is_dir()` é falso para symlink. Custava um `symlink_metadata` por diretório visitado — numa varredura de `C:\`, milhões de syscalls por um `if` morto. Removido, com o contrato de pré-filtragem documentado na própria função.

## Risks / Trade-offs

| Risco | Mitigação |
|---|---|
| Lista de proteção incompleta → apaga pasta vazia que um app esperava | Sem elevação limita o alcance. Nenhum arquivo pode ser perdido (D3). O dano possível é estrutural e reparável recriando a pasta. |
| Sem preview, o usuário não vê o que vai sumir antes | Aceito explicitamente. Justificado por D3: perda de arquivo é impossível. |
| Licença Slint incompatível com distribuição futura | Sinalizado no `proposal.md`. Decidir antes de publicar, não antes de codar. |
| Varredura de disco cheio é lenta | Barra indeterminada + Cancelar responsivo tornam a espera tolerável. Sem otimização até medir. |
| `sysinfo` mudou a API de discos entre versões | Fixar a versão no `Cargo.toml` e conferir a assinatura de `Disks` na release usada. |

## Migration Plan

Não se aplica. Projeto novo, sem usuários, sem dados, sem versão anterior.

## Open Questions

1. **Exclusão de diretórios de sistema** (`C:\Windows`, `Program Files`, `ProgramData`) — assumida como padrão, ver `proposal.md`. Confirmar ou remover.
2. **Licença do Slint** — resolver antes de distribuir.

2b. ~~**`ui/app.ico` com uma única entrada 256×256**~~ — **resolvido.** Agora tem 16/24/32/48/64/128/256, cada uma reamostrada com LANCZOS a partir do PNG de 1024. Reduzir na geração é melhor que deixar o Windows reduzir na hora de desenhar, porque o filtro dele é mais pobre e o ícone tem detalhe fino (a seta de reciclagem) que some se a redução for ruim. Conferido ampliado em 16/32/48.
3. **Drive de rede** — `sysinfo` pode listá-los. Varrer `\\nas\...` é lento e sujeito a timeout. Excluir da lista, ou permitir e aceitar a lentidão? Não bloqueia a primeira versão; decidir se aparecer na prática. Ficou mais provável agora que dá pra selecionar vários discos de uma vez.

4. ~~**Ajuste fino de quais caminhos são verificados**~~ — **resolvido.** O termo cobria três coisas:

   | Leitura | O que faz | Risco | Status |
   |---|---|---|---|
   | **(a) escolher subárvores** | varrer só `D:\Projetos` em vez do disco todo | nenhum — só estreita o alcance | **implementado**, ver D11 |
   | **(b) exclusões próprias** | "nunca toque em `D:\importante`" | nenhum — só adiciona proteção | não pedido |
   | **(c) afrouxar proteções embutidas** | varrer `C:\Windows`, desproteger Known Folders | desfaz a classe A/B, única defesa contra dano estrutural | **fora de escopo** |

   Era (a). Continua valendo: (c) contradiz o Non-Goal "exclusões configuráveis" e remove a rede de segurança de D4/D7 — precisaria de conversa própria antes de virar código.
