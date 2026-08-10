# Folder Sweep

App de janela única para Windows que varre caminhos escolhidos e apaga pastas vazias.

Discos acumulam pastas vazias deixadas por desinstaladores, extração de zip, builds e movimentação de arquivo. O Windows não traz nada que limpe isso, e fazer à mão é inviável porque a vacuidade é **recursiva**: uma pasta que só contém pastas vazias também fica vazia depois que os filhos somem.

```
A/                    A/
└── B/        →       └── B/       →    A/     →    (nada)
    └── C/  (vazia)
```

O Folder Sweep resolve a cascata inteira numa passada só, percorrendo de baixo para cima.

---

## Aviso

**A remoção é permanente.** Não usa Lixeira, não tem quarentena, não tem undo.

Isso é menos assustador do que parece, por um motivo estrutural: a remoção usa `std::fs::remove_dir`, que o **kernel recusa** se a pasta não estiver vazia. Não existe caminho de código que apague um arquivo — nem sob condição de corrida, com um arquivo criado no microssegundo entre a checagem e a remoção. O que se pode perder é uma *pasta vazia que algo esperava encontrar*, e é contra isso que a lista de proteção existe.

Ainda assim: **teste numa pasta descartável antes de soltar num disco inteiro.**

---

## Como funciona

1. O app lista os discos montados como raízes de uma árvore.
2. Você expande até onde quiser e **marca** os caminhos a limpar — um ou vários, em qualquer nível, em qualquer combinação de discos.
3. Opcionalmente marque **Gravar log** — o Iniciar então pergunta onde salvar o `.txt` antes de tocar em qualquer pasta.
4. Iniciar varre os caminhos marcados em sequência, numa thread separada.
5. Barra indeterminada mais contadores ao vivo: analisadas, apagadas, erros, tempo decorrido.
6. Cancelar interrompe entre uma remoção e a próxima.

```
┌──────────────────────────────────────────────┐
│  Marque os caminhos que quer limpar          │
│ ┌──────────────────────────────────────────┐ │
│ │ ▾ D:\           SSD_3          931 GB    │ │
│ │    ▸ Downloads                           │ │
│ │    ▾ Projetos                            │ │
│ │       ✓ app-antigo                       │ │
│ │       ▸ foldersweep                      │ │
│ │    ▸ Windows        protegido            │ │
│ │ ▸ E:\           USB    57 GB  removível  │ │
│ └──────────────────────────────────────────┘ │
│  1 caminho marcado             [x] Gravar log│
│  [ Iniciar ]  [ Cancelar ]                   │
│  ░░▓▓▓▓░░░░░░░░░░░░░                         │
│  12.483 analisadas · 213 apagadas · 0 erros · 2:07
│  D:\Projetos\algum\caminho\longo…            │
└──────────────────────────────────────────────┘
```

A árvore carrega **um nível por vez**, sob demanda. O app nunca enumera um disco inteiro antes de começar — é justamente o que a barra indeterminada existe para evitar.

Cancelar é seguro em qualquer momento. A unidade de trabalho é uma única chamada `remove_dir`, atômica e independente: não existe "meio de uma remoção", nem estado parcial, nem rollback. Parar em 213 de 500 deixa 213 removidas e 287 no disco.

### O log

Com **Gravar log** marcada, o Iniciar abre o diálogo de salvar do Windows e só depois começa a varrer. O arquivo é escrito **conforme a análise anda**, não montado no fim: cada pasta vira uma linha assim que é tocada, e o buffer é descarregado no mesmo ritmo em que a janela atualiza os contadores. Fechar o app no meio da corrida deixa no disco tudo o que já tinha acontecido.

```
Folder Sweep — log de varredura
Iniciado em: 2026-08-09 14:33:02
Caminhos marcados:
  D:\Projetos

ANALISADA  D:\Projetos\app-antigo
ANALISADA  D:\Projetos\app-antigo\build
APAGADA    D:\Projetos\app-antigo\build
ERRO       D:\Projetos\travada — Acesso negado. (os error 5)

Concluído em: 2026-08-09 14:36:09
Duração: 3:07
Resumo: 12483 analisadas · 213 apagadas · 1 erros
```

Ordem cronológica com tag por linha, e não seções agrupadas: em stream as três categorias chegam intercaladas, então agrupar exigiria segurar tudo em memória — exatamente o que escrever ao vivo evita. Um `findstr APAGADA` recupera o agrupamento quando ele fizer falta.

Se o destino não puder ser criado, ou se o diálogo for fechado sem escolher, **a varredura não começa**: quem marcou a caixa pediu o registro, e apagar sem ele é o oposto do pedido. Se a escrita falhar no meio, aí sim a varredura segue — o disco já foi alterado, parar na metade deixaria um estado que ninguém pediu — e a janela avisa que o log ficou incompleto.

---

## O que ele não apaga

Duas classes de proteção, com semânticas diferentes. Confundi-las quebra o app nos dois sentidos: tratar A como subárvore o deixaria inerte; tratar B como caminho exato o deixaria destrutivo.

### Classe A — caminho exato

Não apaga **esta** pasta, mas varre normalmente **dentro** dela.

- A raiz de cada caminho que você marcou.
- Known Folders do perfil: Desktop, Documents, Downloads, Music, Videos, Pictures, Templates, Public, home e os diretórios de config/data/cache.

Resolvidos em tempo de execução pelo caminho real, nunca por nome — `D:\Projetos\app\Documents` é lixo legítimo e é apagado; `C:\Users\você\Documents` não é.

E a proteção **não se propaga para o conteúdo**: uma pasta vazia dentro de `Downloads` é removida normalmente. É justo onde pasta vazia mais nasce.

### Classe B — subárvore inteira

Não apaga nada aqui dentro e nem desce. Aparece na árvore esmaecida, rotulada `protegido`, sem seta e sem clique.

- `%SystemRoot%`, `%ProgramFiles%`, `%ProgramFiles(x86)%`, `%ProgramW6432%`, `%ProgramData%` — lidos do ambiente, não fixos em `C:`, porque o Windows não mora obrigatoriamente lá.
- `$RECYCLE.BIN` e `System Volume Information`, em qualquer volume.
- Qualquer subárvore `.git` — o git mantém vários diretórios vazios de propósito.
- Reparse points (junctions e symlinks): nunca seguidos, nunca removidos. Seguir poderia levar a varredura para fora do volume marcado.

Comparação é case-insensitive e **componente a componente** — `C:\Windows2` não casa com `C:\Windows`.

### O que conta como vazia

Vacuidade estrita. Uma pasta com apenas `desktop.ini`, `Thumbs.db` ou um arquivo de zero byte **não** está vazia e não é tocada. Isso não é código: o kernel recusa a remoção.

### Sem elevação

O app não pede admin, de propósito. Sem admin ele **não consegue** apagar dentro do diretório do Windows mesmo que a lista de proteção tenha um furo. Elevar transformaria um bug de filtro em dano ao sistema. Diretório inacessível é contado como erro e pulado; a varredura nunca aborta por causa de um.

---

## Build

Precisa do toolchain **Windows** (`x86_64-pc-windows-msvc`). O repositório pode viver num caminho acessível pelo WSL, mas compilar pelo `cargo` do WSL produz um binário Linux, que não serve.

```powershell
cargo build --release
cargo test
```

Sem dependência de sistema além do toolchain. O `build.rs` compila o `.slint` e embute o ícone multi-resolução no executável.

### Dependências

| Crate | Para quê |
|---|---|
| `slint` | GUI, estilo `fluent` (segue o tema claro/escuro do Windows) |
| `sysinfo` | enumerar volumes montados |
| `dirs` | resolver Known Folders pelo caminho real, e a pasta Documentos como destino inicial do log |
| `rfd` | diálogo nativo de salvar arquivo |
| `chrono` | data e hora locais no cabeçalho e no rodapé do log |
| `slint-build` | compilar `ui/app.slint` (build) |
| `winresource` | embutir `ui/app.ico` no executável (build) |

A travessia e o cancelamento não usam dependência nenhuma: `std::fs::read_dir` e `AtomicBool`.

---

## Estrutura

```
src/
  main.rs      wiring da UI, worker thread, estado da árvore e da seleção
  sweep.rs     recursão pós-ordem, dedupe de raízes, cancelamento, throttle
  protect.rs   classes A e B, comparação case-insensitive de caminho
  log.rs       arquivo de log escrito ao vivo, cabeçalho e rodapé
  tree.rs      listagem de um nível, contagem de descendentes
  disks.rs     enumeração de volumes
ui/
  app.slint    janela, árvore, botões, barra, linhas de status
openspec/      proposta, design e specs da mudança
```

~1.870 linhas, 33 testes.

---

## Decisões de projeto

Registradas em `openspec/changes/limpar-pastas-vazias/design.md`, com as alternativas descartadas e o porquê. As que mais afetam o comportamento:

- **`remove_dir` é o oráculo de vacuidade.** Não há checagem prévia — tenta remover e interpreta o erro. Uma syscall por pasta em vez de duas, corrida impossível, e a regra da vacuidade estrita sai de graça, decidida pelo kernel.
- **Pós-ordem manual, não `walkdir`.** A combinação `contents_first` + `filter_entry` do walkdir 2.5 corrompe a travessia: `skip_current_dir` faz `pop()` na pilha de leitura atual, que em pós-ordem pode já pertencer a um diretório-irmão. Recursão manual sobre `read_dir` evita o bug e dispensa a dependência.
- **Guard da seleção completa, travessia da lista deduplicada.** Marcar `D:\` e `D:\Projetos` percorre a subárvore uma vez só, mas **ambos** continuam protegidos de remoção. Escrito na ordem inversa, `D:\Projetos` seria apagada se ficasse vazia.
- **Seleção vive fora do modelo de linhas.** Colapsar um nó remove as linhas dos filhos; se a marcação morasse na linha, seria perdida. Um `HashSet<PathBuf>` a mantém viva.
- **Nada de glifo Unicode acima de Latin-1 na UI.** `▸`, `▾` e `✓` estão no Segoe UI Symbol, não no Segoe UI base, e o fallback de fonte do Slint não os alcança — renderizavam como quadrado de tofu. Setas e check são `Path` vetorial.
- **Cores saem do `Palette`, nunca hex fixo.** Hex assume um tema e fica ilegível no outro.

---

## Estado

Funcional e testado em unidade. Falta a verificação manual de ponta a ponta — rodar o executável contra árvores reais e conferir o resultado no disco.

Pendências conhecidas, detalhadas em `openspec/changes/limpar-pastas-vazias/tasks.md`:

- Expandir a árvore faz I/O bloqueante na thread de UI; pasta com dezenas de milhares de filhos, ou volume de rede lento, trava a janela durante a leitura.
- Não há confirmação antes de iniciar. Um clique errado no Iniciar com um disco inteiro marcado começa a apagar imediatamente.
- Comparação de caminho não cobre nomes curtos 8.3, `subst` nem forma UNC.
- `.cargo/config.toml` usa `-C target-cpu=native`: correto para build pessoal, mas o executável aborta com instrução ilegal se copiado para uma máquina de CPU mais antiga.

## Licença

Indefinida. O Slint é triplo-licenciado (GPLv3 / royalty-free / comercial); a royalty-free cobre aplicação desktop, mas as condições precisam ser lidas antes de qualquer distribuição.
