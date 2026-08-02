## ADDED Requirements

### Requirement: Enumeração dos discos montados

O app MUST listar os volumes montados na inicialização, exibindo para cada um o ponto de montagem, o rótulo do volume e o espaço total.

#### Scenario: Discos aparecem ao abrir o app

- **WHEN** o app é iniciado
- **THEN** a janela exibe uma linha por volume montado
- **AND** cada linha mostra ponto de montagem (ex.: `D:\`), rótulo (ex.: `SSD_3`) e espaço total legível (ex.: `931 GB`)

#### Scenario: Volume removível é identificado

- **WHEN** um volume removível (pendrive, HD externo) está montado
- **THEN** ele aparece na lista marcado como removível

#### Scenario: Nenhum volume elegível

- **WHEN** a enumeração não retorna nenhum volume utilizável
- **THEN** a lista fica vazia com mensagem explicativa
- **AND** o botão Iniciar permanece desabilitado

### Requirement: Marcação múltipla

O app MUST permitir marcar um ou mais caminhos ao mesmo tempo, em qualquer nível da árvore e em qualquer combinação de discos. Cada caminho alterna entre marcado e não marcado de forma independente.

#### Scenario: Marcar um caminho não desmarca o anterior

- **WHEN** o usuário marca `D:\` e depois marca `E:\`
- **THEN** `D:\` e `E:\` ficam ambos marcados

#### Scenario: Marcar caminhos em discos diferentes

- **WHEN** o usuário marca `C:\Users\<user>\Downloads` e `D:\Projetos`
- **THEN** ambos ficam marcados
- **AND** a varredura roda sobre os dois

#### Scenario: Clicar de novo desmarca

- **WHEN** o usuário clica num caminho já marcado
- **THEN** aquele caminho fica desmarcado
- **AND** os demais marcados permanecem marcados

#### Scenario: Iniciar exige ao menos um caminho

- **WHEN** nenhum caminho está marcado
- **THEN** o botão Iniciar está desabilitado

#### Scenario: Contagem de marcados é exibida

- **WHEN** o usuário tem caminhos marcados
- **THEN** a UI mostra quantos caminhos estão marcados

### Requirement: Árvore expansível de caminhos

A lista MUST ser uma árvore: cada disco expande para mostrar seus subdiretórios, que por sua vez expandem, sem limite de profundidade. O usuário MUST poder marcar qualquer nó da árvore — disco ou pasta em qualquer nível — como raiz de varredura.

A expansão MUST carregar sob demanda, um nível por vez. O app MUST NOT enumerar o disco inteiro para montar a árvore.

Isto **estreita** o alcance da varredura. O app MUST NOT oferecer nenhum meio de afrouxar as proteções embutidas descritas em `empty-folder-sweep`.

#### Scenario: Expandir um disco mostra os subdiretórios imediatos

- **WHEN** o usuário expande `D:\`
- **THEN** os subdiretórios imediatos de `D:\` aparecem indentados logo abaixo
- **AND** apenas um nível é lido, não a árvore inteira
- **AND** arquivos não aparecem, só pastas

#### Scenario: Expandir mais fundo

- **GIVEN** `D:\` expandido
- **WHEN** o usuário expande `D:\Projetos`
- **THEN** os subdiretórios de `D:\Projetos` aparecem indentados abaixo dela
- **AND** os irmãos de `D:\Projetos` continuam nas posições relativas

#### Scenario: Colapsar remove os descendentes exibidos

- **GIVEN** `D:\` expandido com vários níveis abertos dentro
- **WHEN** o usuário colapsa `D:\`
- **THEN** todas as linhas descendentes de `D:\` somem da lista
- **AND** as linhas de outros discos não são afetadas

#### Scenario: Marcação sobrevive a colapsar e reexpandir

- **GIVEN** `D:\Projetos` marcado, dentro de `D:\` expandido
- **WHEN** o usuário colapsa `D:\` e expande de novo
- **THEN** `D:\Projetos` continua marcado
- **AND** a contagem de caminhos marcados não mudou

#### Scenario: Varredura roda só nos caminhos marcados

- **GIVEN** apenas `D:\Projetos` marcado, nenhum disco marcado
- **WHEN** o usuário inicia
- **THEN** a varredura percorre somente `D:\Projetos` e seus descendentes
- **AND** nada fora dessa subárvore é visitado

#### Scenario: Caminho marcado nunca é removido

- **GIVEN** `D:\Projetos` marcado
- **WHEN** a varredura termina e `D:\Projetos` ficou vazia
- **THEN** `D:\Projetos` permanece
- **AND** as pastas vazias dentro dela foram removidas

#### Scenario: Pasta sem subdiretórios perde a seta

- **WHEN** o usuário expande uma pasta que não tem subpastas, ou que não pode ser lida
- **THEN** nenhuma linha filha aparece
- **AND** a seta de expansão some daquela linha

#### Scenario: Subárvore protegida aparece marcada e inerte

- **WHEN** a árvore mostra uma pasta da classe B, como `C:\Windows` ou `.git`
- **THEN** ela aparece esmaecida e rotulada como protegida
- **AND** não pode ser marcada
- **AND** não pode ser expandida

#### Scenario: Árvore congelada durante a varredura

- **WHEN** uma varredura está em andamento
- **THEN** expandir, colapsar e marcar ficam desabilitados

### Requirement: Alvo aninhado em outro alvo

Quando um alvo selecionado está contido em outro alvo selecionado, o app MUST varrer a subárvore compartilhada uma única vez, e MUST proteger de remoção **todos** os alvos selecionados — inclusive o aninhado, que não é percorrido como raiz própria.

#### Scenario: Subárvore compartilhada é percorrida uma vez só

- **GIVEN** `D:\` e `D:\Projetos` ambos selecionados
- **WHEN** a varredura roda
- **THEN** `D:\Projetos` é percorrida uma única vez
- **AND** o contador de analisadas não conta a mesma pasta duas vezes

#### Scenario: Alvo aninhado continua protegido

- **GIVEN** `D:\` e `D:\Projetos` ambos selecionados
- **WHEN** `D:\Projetos` fica vazia durante a varredura de `D:\`
- **THEN** `D:\Projetos` NÃO é removida, porque o usuário a nomeou como alvo

#### Scenario: Alvos duplicados exatos colapsam

- **GIVEN** o mesmo caminho selecionado duas vezes
- **WHEN** a varredura roda
- **THEN** ele é percorrido uma vez só

### Requirement: Caminhos varridos em sequência com contadores acumulados

Quando mais de um caminho está marcado, o app MUST varrê-los em sequência numa única corrida, e os contadores e o tempo decorrido MUST acumular ao longo de toda a corrida em vez de reiniciar a cada caminho.

#### Scenario: Contadores não zeram ao trocar de caminho

- **GIVEN** dois caminhos marcados
- **WHEN** a varredura termina o primeiro e começa o segundo
- **THEN** os contadores de analisadas, apagadas e erros continuam de onde estavam
- **AND** o tempo decorrido continua contando desde o início da corrida

#### Scenario: Cancelar interrompe a corrida inteira

- **GIVEN** dois caminhos marcados e a varredura no primeiro
- **WHEN** o usuário cancela
- **THEN** o segundo caminho não é varrido

#### Scenario: Cada caminho marcado é protegido de remoção

- **GIVEN** `D:\` e `E:\Backups` marcados
- **WHEN** a varredura roda
- **THEN** nem `D:\` nem `E:\Backups` são removidos

### Requirement: Árvore congelada durante a varredura

Enquanto uma varredura está em andamento, o app MUST impedir alterações na árvore, para que o conjunto de raízes não mude no meio da travessia.

#### Scenario: Interação bloqueada durante execução

- **WHEN** uma varredura está em andamento
- **THEN** marcar, expandir e colapsar ficam desabilitados

#### Scenario: Interação liberada ao terminar

- **WHEN** a varredura termina, é cancelada ou falha
- **THEN** a árvore volta a aceitar marcação e expansão
- **AND** os caminhos marcados antes da varredura continuam marcados
