# empty-folder-sweep

## Purpose

Percorrer os caminhos marcados de baixo para cima, remover pastas vazias permanentemente, respeitar as duas classes de caminho protegido, reportar progresso sem bloquear a UI e obedecer cancelamento cooperativo.

A remoção usa `std::fs::remove_dir`, que o kernel recusa em pasta não vazia. Nenhum caminho de código pode apagar um arquivo.

## Requirements

### Requirement: Travessia bottom-up com cascata

A varredura MUST visitar os diretórios em pós-ordem (filhos antes do pai), de modo que uma pasta cujo único conteúdo eram pastas vazias seja ela própria removida na mesma passada. O app MUST NOT executar múltiplas passadas até estabilizar.

#### Scenario: Cadeia aninhada some numa passada

- **GIVEN** a estrutura `A/B/C` onde `C` está vazia e `A` e `B` não contêm nada além do filho
- **WHEN** a varredura roda uma única vez
- **THEN** `C`, `B` e `A` são todas removidas
- **AND** nenhuma segunda passada é executada

#### Scenario: Pai com arquivo sobrevive

- **GIVEN** `A/` contém `nota.txt` e a pasta vazia `A/B`
- **WHEN** a varredura roda
- **THEN** `A/B` é removida
- **AND** `A` permanece

### Requirement: Vacuidade decidida pelo kernel

O app MUST tentar a remoção diretamente com `std::fs::remove_dir` e tratar o erro `DirectoryNotEmpty` como resultado normal, em vez de checar a vacuidade com uma leitura de diretório prévia. Isso elimina a janela de corrida entre checar e apagar.

#### Scenario: Pasta com arquivo oculto não é apagada

- **GIVEN** uma pasta contendo apenas `desktop.ini`
- **WHEN** a varredura tenta removê-la
- **THEN** a remoção falha com `DirectoryNotEmpty`
- **AND** a pasta permanece intacta
- **AND** isso NÃO é contabilizado como erro

#### Scenario: Pasta com arquivo de zero bytes não é apagada

- **GIVEN** uma pasta contendo apenas um arquivo de 0 bytes
- **WHEN** a varredura tenta removê-la
- **THEN** a pasta permanece intacta

#### Scenario: Arquivo criado durante a corrida não é perdido

- **GIVEN** uma pasta vazia que recebe um arquivo novo imediatamente antes da tentativa de remoção
- **WHEN** a varredura tenta removê-la
- **THEN** o kernel recusa a remoção
- **AND** nenhum arquivo é perdido

### Requirement: Remoção permanente

Pastas removidas MUST ser apagadas permanentemente. O app MUST NOT usar a Lixeira, área de quarentena ou qualquer mecanismo de undo.

#### Scenario: Pasta removida não vai para a Lixeira

- **WHEN** uma pasta vazia é removida
- **THEN** ela não aparece na Lixeira do Windows

### Requirement: Caminhos protegidos

O app MUST proteger os caminhos abaixo, e MUST tratá-los em **duas classes com semânticas diferentes**. Confundi-las quebra o app: tratar a classe A como subárvore tornaria o app inerte, porque protegeria todo o conteúdo do perfil do usuário.

**Classe A — caminho exato.** O app MUST NOT remover estas pastas, mas MUST percorrer o interior delas normalmente e remover pastas vazias lá dentro. A comparação MUST ser igualdade de caminho canônico, nunca prefixo, nunca nome.

1. A raiz do volume (ex.: `D:\`).
2. Known Folders do usuário, resolvidos em tempo de execução para caminho absoluto: Desktop, Documents, Downloads, Music, Videos, Pictures, Templates, Public, home, e os diretórios de config/data/cache do perfil.

**Classe B — subárvore inteira.** O app MUST NOT remover nada aqui dentro e MUST NOT descer nestes diretórios. A travessia MUST podar antes de entrar, não rejeitar entrada por entrada depois de descer.

3. `C:\Windows`, `C:\Program Files`, `C:\Program Files (x86)`, `C:\ProgramData`.
4. `$RECYCLE.BIN` e `System Volume Information` em qualquer volume.
5. Qualquer subárvore `.git`.
6. Reparse points — junctions e symlinks para diretório.

#### Scenario: Known Folder vazio sobrevive

- **GIVEN** `C:\Users\<user>\Music` existe e está vazia
- **WHEN** a varredura passa por ela
- **THEN** a pasta NÃO é removida

#### Scenario: Pasta vazia DENTRO de um Known Folder É removida

- **GIVEN** a pasta vazia `C:\Users\<user>\Downloads\zip-extraido\vazia`
- **WHEN** a varredura passa por ela
- **THEN** a pasta É removida
- **AND** `C:\Users\<user>\Downloads` permanece
- **AND** a proteção de Known Folder NÃO se propaga para o conteúdo

#### Scenario: Subárvore de sistema não é percorrida

- **GIVEN** o volume varrido é `C:\` e contém `C:\Windows` com milhares de subdiretórios
- **WHEN** a varredura roda
- **THEN** a travessia poda `C:\Windows` sem descer nele
- **AND** nenhuma entrada dentro de `C:\Windows` é visitada

#### Scenario: Pasta homônima fora do Known Folder é apagada

- **GIVEN** a pasta vazia `D:\Projetos\meu-app\Documents`
- **WHEN** a varredura passa por ela
- **THEN** a pasta É removida, porque a proteção casa por caminho real e não por nome

#### Scenario: Diretório interno do git sobrevive

- **GIVEN** `D:\repo\.git\refs\tags` existe e está vazia
- **WHEN** a varredura passa por ela
- **THEN** a pasta NÃO é removida

#### Scenario: Raiz do volume nunca é removida

- **GIVEN** um volume montado sem nenhum conteúdo
- **WHEN** a varredura termina
- **THEN** a raiz do volume permanece

#### Scenario: Junction não é seguida

- **GIVEN** um reparse point apontando para fora do volume varrido
- **WHEN** a varredura o encontra
- **THEN** a travessia não desce por ele
- **AND** o próprio reparse point não é removido

### Requirement: Cancelamento cooperativo

O app MUST fornecer cancelamento que interrompe a varredura em um limite entre remoções. A unidade de trabalho é uma única chamada `remove_dir`, portanto não existe estado parcial nem rollback.

#### Scenario: Cancelar interrompe rápido

- **WHEN** o usuário aciona Cancelar durante a varredura
- **THEN** a varredura para antes da próxima tentativa de remoção
- **AND** a UI volta ao estado ocioso

#### Scenario: Trabalho já feito permanece feito

- **GIVEN** 213 de 500 pastas já foram removidas
- **WHEN** o usuário cancela
- **THEN** as 213 permanecem removidas
- **AND** as restantes permanecem no disco
- **AND** nenhuma restauração é tentada

#### Scenario: Cancelar sem varredura ativa

- **WHEN** nenhuma varredura está em andamento
- **THEN** o botão Cancelar está desabilitado

### Requirement: Erros não abortam a varredura

Falhas de permissão e de I/O MUST ser contadas e ignoradas individualmente. Uma pasta que não pôde ser removida ou lida MUST NOT interromper a travessia.

#### Scenario: Acesso negado é contado e a varredura segue

- **GIVEN** o disco contém diretórios que o usuário não tem permissão de ler
- **WHEN** a varredura os encontra
- **THEN** o contador de erros incrementa
- **AND** a varredura continua nos diretórios restantes
- **AND** a varredura termina normalmente

#### Scenario: Volume some no meio da varredura

- **GIVEN** um volume removível é desconectado durante a varredura
- **WHEN** a travessia falha
- **THEN** a varredura termina com mensagem de erro em vez de travar ou crashar

### Requirement: Sem elevação

O app MUST NOT solicitar elevação de privilégio. Rodar sem admin é uma medida de segurança deliberada: limita o estrago possível a caminhos que o usuário já pode modificar.

#### Scenario: Roda como usuário comum

- **WHEN** o app é iniciado
- **THEN** nenhum prompt de UAC aparece
- **AND** diretórios inacessíveis são contados como erro e pulados

### Requirement: Progresso indeterminado com contadores ao vivo

O app MUST exibir uma barra de progresso indeterminada durante a varredura, e MUST NOT executar uma fase de contagem prévia para calcular percentual. Contadores textuais fornecem a evidência de avanço real.

#### Scenario: Barra anima durante a execução

- **WHEN** a varredura está em andamento
- **THEN** a barra de progresso anima continuamente
- **AND** nenhum percentual é exibido

#### Scenario: Contadores avançam ao vivo

- **WHEN** a varredura está em andamento
- **THEN** a UI mostra o total de pastas analisadas, o total de pastas removidas, o total de erros e o tempo decorrido

#### Scenario: Pasta atual fica em linha própria

- **WHEN** a varredura está em andamento
- **THEN** o caminho da pasta sendo verificada aparece numa linha separada, abaixo da linha de contadores
- **AND** caminhos longos são elididos em vez de esticar a janela

#### Scenario: Pasta podada conta como analisada

- **GIVEN** uma subárvore protegida da classe B, como `.git`
- **WHEN** a varredura a encontra e poda
- **THEN** a própria pasta podada conta uma vez em analisadas
- **AND** o conteúdo dela não conta, porque não foi descido

#### Scenario: Tempo decorrido conta a corrida inteira

- **WHEN** a varredura roda sobre múltiplos discos
- **THEN** o tempo decorrido é medido desde o início da corrida, não desde o início do disco atual
- **AND** é exibido como `M:SS`, ou `H:MM:SS` quando passa de uma hora

#### Scenario: Atualizações são limitadas

- **WHEN** a travessia produz entradas mais rápido do que a UI consegue renderizar
- **THEN** as atualizações da UI são limitadas a no máximo uma a cada ~100 ms
- **AND** o event loop não é inundado

#### Scenario: Resumo ao terminar

- **WHEN** a varredura termina ou é cancelada
- **THEN** a barra para
- **AND** o resumo final mostra pastas analisadas, removidas, erros e tempo total
- **AND** a linha de pasta atual fica vazia

### Requirement: UI nunca bloqueia

A varredura MUST rodar fora da thread de UI. A janela MUST permanecer responsiva — arrastável, redimensionável, com o botão Cancelar clicável — durante toda a execução.

#### Scenario: Janela responde durante varredura de disco cheio

- **WHEN** uma varredura de disco inteiro está em andamento
- **THEN** a janela pode ser movida e redimensionada
- **AND** Cancelar responde ao clique

#### Scenario: Estados dos controles

- **WHEN** a varredura começa
- **THEN** Iniciar fica desabilitado, Cancelar habilitado e a lista de discos desabilitada
- **WHEN** a varredura termina ou é cancelada
- **THEN** Iniciar fica habilitado, Cancelar desabilitado e a lista de discos habilitada
- **AND** a seleção de discos feita antes da varredura é preservada

#### Scenario: Fechar a janela durante a varredura

- **WHEN** o usuário fecha a janela com a varredura em andamento
- **THEN** o cancelamento é sinalizado
- **AND** o processo termina sem travar esperando a travessia
