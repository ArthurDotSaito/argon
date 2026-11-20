# Argon

Aplicativo desktop nativo (Rust + GPUI Kit) que mostra os Pull Requests e as issues do GitHub como uma caixa de e-mail.

## Pré-requisitos

- Rust 1.96 (fixado em `rust-toolchain.toml`)
- Visual Studio Build Tools 2022 (MSVC)
- GitHub CLI (`gh`) autenticado (`gh auth login`)

## Executar

```
cargo run -p argon-app
```

## Agentes

O Argon abre uma sessão de revisão por agente de código instalado na máquina, pelo botão **Agent Review** no PR. Cada agente precisa do CLI correspondente instalado e autenticado; as versões abaixo são as validadas nesta branch.

| Agente | Protocolo | CLI exigido | Versão validada |
| --- | --- | --- | --- |
| Claude Code | nativo (`claude -p` com stream-json, padrão) ou ACP | `claude` | 2.1.316 |
| Codex | nativo (`codex app-server`, padrão) ou ACP | `codex` | codex-cli 0.157.0 |
| OpenCode | nativo (`opencode serve` privado, padrão) ou ACP | `opencode` | 2.2.5 |
| Command Code | ACP | `command-code` | via ACP |

- **Nativo**: o Argon fala direto com o CLI. **ACP**: o Argon fala com um adapter ACP, que o `npx` baixa na hora (Claude Code e Codex) ou que já vem no CLI (OpenCode e Command Code). O ACP continua como único caminho do Command Code e como fallback por agente, na opção "Usar adapter ACP (legado)" das configurações.
- Claude Code, Codex e OpenCode rodam no driver nativo por padrão. O adapter ACP de cada um continua disponível na opção "Usar adapter ACP (legado)".
- No Claude Code nativo, se o processo do CLI sair entre um turno e outro, a sessão continua: a próxima mensagem retoma a conversa com `--resume`.

### Segurança

A configuração de agente que vem dentro do PR revisado não é confiável. Cada driver nativo a ignora:

- **Claude Code**: `--setting-sources user --strict-mcp-config --permission-mode default`, sem settings nem MCP do projeto.
- **OpenCode**: `OPENCODE_CONFIG_DIR` apontando para uma pasta do Argon e `OPENCODE_DISABLE_PROJECT_CONFIG` ligado. O Argon mantém nessa pasta um `opencode.json` com `"permission": {"*": "ask"}`, então toda ferramenta pede permissão ao Argon. A mesma política vai também em `OPENCODE_CONFIG_CONTENT`, que tem a maior precedência, e o Argon sempre sobrescreve essa variável, mesmo que o env do agente a defina.
- **Codex**: o diretório da revisão entra em cada thread com `trust_level = "untrusted"` e `approvalsReviewer: "user"`, então as aprovações que o Codex pede chegam ao Argon.

O que cada driver nativo pede ao Argon:

- **Claude Code e OpenCode**: comandos, edições e buscas na web viram pedido de permissão para o Argon. No OpenCode, negar um card encerra o turno; para continuar, mande outra mensagem ao agente. No Claude Code, as regras `permissions.allow` do próprio usuário em `~/.claude/settings.json` continuam valendo nas revisões do Argon: o que elas liberam roda sem card.
- **Codex**: em Supervisionado e Aceitar edições, todo comando do Codex vira card, inclusive leituras como `git diff` e `rg`, porque a sandbox somente leitura do Codex no Windows não impede ler fora da worktree. "Permitir nesta sessão" reduz a repetição, mas só aparece em comandos que o Codex classifica inteiramente como leitura, listagem ou busca; o Codex guarda essa aprovação em memória, só na sessão aberta. Comandos que podem alterar algo (como `npm test`), escritas no stdin de um processo e pedidos de rede ou de permissões extras sempre pedem card. O card mostra a pasta do comando quando ela não é a worktree da revisão. Nesses dois modos a busca na web do Codex fica desligada (`web_search = "disabled"`). Como no Claude Code, as regras `allow` do próprio usuário em `~/.codex/rules` (`prefix_rule(..., decision = "allow")`) continuam valendo nas revisões do Argon: o que elas liberam roda sem card e fora da sandbox. O `codex app-server` não tem opção para ignorá-las (o `--ignore-rules` só existe no `codex exec`). Em Acesso total os comandos rodam sem card.
- Os títulos dos cards passam por um único filtro: caracteres de controle, bidi e de largura zero viram espaço, e o texto é cortado em 2000 caracteres (comandos, rede e outros) ou 300 (leituras, buscas e edições), sempre com " …" visível quando há corte.

Modos de permissão, por agente:

- **Supervisionado**: leituras e buscas dentro da worktree da revisão são aprovadas sem card; o resto que o agente pede ao Argon aparece em um card para aprovar ou negar.
- **Aceitar edições**: leituras, buscas e edições dentro da worktree da revisão são aprovadas automaticamente; o resto continua pedindo.
- **Acesso total**: aprova tudo sem mostrar o card. O agente lê código de PR de terceiros, então use só se aceitar o risco. No Codex exige a sandbox do Windows pronta; sem ela a sessão abre em Supervisionado. Sair do Acesso total no meio de um turno do Codex interrompe o turno e encerra o processo do Codex, com os processos em segundo plano que ele abriu; a próxima mensagem retoma a conversa já no modo mais restrito.

"Permitir nesta sessão" vale só para a sessão aberta, fica em memória (não é gravado em disco) e só vale para leituras, buscas e edições dentro da worktree da revisão. A exceção são os comandos do Codex, descritos acima.

### Agent Plan

Na aba da issue, o botão **Agent Plan** (também na paleta e no atalho `a`) abre o mesmo diálogo do Agent Review, com agente, modelo e um template de plano. O agente lê a issue e o código numa worktree da branch padrão do repositório e propõe um plano de solução.

- **Somente leitura**: edição, apagar ou mover arquivo, troca de modo e pedido de permissão extra são negados sem card. Comandos pedem card. O modo fica fixo em Supervisionado.
- **Rascunho**: o plano aparece num cartão editável na aba da sessão e num aviso na aba da issue. Nada vai para o GitHub sem o clique em "Publicar na issue".
- **Publicação**: cada publicação é um comentário novo na issue, com um rodapé que diz que o plano saiu do Agent Plan, com qual agente e modelo, e quem revisou. O rascunho publicado fica travado; uma versão nova do agente vira outro rascunho.

Limitação: as regras `allow` do próprio usuário em `~/.codex/rules` e as regras `Bash(...)` em `~/.claude/settings.json` continuam valendo na sessão de plano. Os comandos que elas liberam rodam sem card e podem escrever na worktree.

### Summarize

No PR, o botão **Summarize** (também na paleta e no atalho `m`) abre uma sessão que explica o projeto e o que o PR faz. Funciona em PR aberto, fechado ou mergeado.

- **Somente leitura**, com as mesmas regras do Agent Plan.
- **Projetos relacionados**: cada repositório tem uma página de projeto (grupo "Projetos" na barra lateral, ou a linha "Projeto" nas propriedades do PR) onde se listam até 8 repositórios ligados a ele por fila, rede ou pacote. O Argon faz checkout da branch padrão de cada um numa pasta `rel-…` ao lado das worktrees do repositório, e o agente lê essas pastas junto com a worktree do PR. Um relacionado que falha no checkout é avisado ao agente e o resto segue. O relacionado entra como `owner/nome` ou pela URL do GitHub, ou como uma pasta local escolhida no seletor de pastas; a pasta local é lida como está, inclusive mudanças não commitadas, sem checkout, e nunca é apagada pelo Argon.
- **Visão do projeto**: na primeira vez num repositório o agente lê o projeto inteiro e devolve o texto (objetivo, finalidade, arquitetura, integrações) e o diagrama da arquitetura. A visão fica salva na página do projeto; nos PRs seguintes o agente parte dela, a menos que "Reler o projeto inteiro" esteja marcado no diálogo, o que substitui a visão salva.
- **Resumo do PR**: fica só no Argon, num cartão da sessão e num aviso no PR, que fica amarelo quando o PR recebe commits depois do resumo. Nada é publicado no GitHub.
- **Diagrama**: a aba Arquitetura da página do projeto desenha quem chama o projeto, os componentes dele e do que ele depende, com zoom e rolagem. No cartão do resumo aparece o recorte com o que o PR toca. O diagrama vem como dados do agente e é validado antes de ser guardado (até 40 nós e 80 ligações).

Leitura nas pastas relacionadas, por agente:

| Agente | Leitura numa pasta relacionada |
| --- | --- |
| Claude Code (nativo) | Sem card. Cada pasta vai como `--add-dir`. |
| Codex (nativo) | Pede card: toda leitura do Codex é um comando. |
| OpenCode (nativo) | Pede um card de diretório externo antes da leitura; a leitura em si passa. |
| ACP | Sem card, pela regra do Argon. |

As pastas relacionadas só liberam leitura e busca, só em sessão somente leitura, e só se forem pastas `rel-…` existentes depois de resolver o caminho, ou pastas locais existentes que não sejam a raiz do disco nem a pasta do usuário. Editar, apagar ou mover nelas é negado. O Claude Code pode carregar instruções (`CLAUDE.md`, skills) das pastas adicionadas com `--add-dir`; são a branch padrão de repositórios que o próprio usuário listou.

### Uso do plano

Os cartões de agente mostram quanto do plano já foi usado:

- **Claude Code e Codex**: lido pelo CLI instalado, sem gastar tokens.
- **OpenCode Go**: só é consultado com consentimento explícito do usuário.
- **Command Code**: mostra um link para a página de uso (https://commandcode.ai/usage).

## Notas para desenvolvedores

`run_session` roda uma sessão de revisão completa contra um PR real, pela linha de comando, e permite escolher o transporte:

```
cargo run -p argon-agents --example run_session -- <claude|codex|opencode|command-code> <owner/repo#n> [modelo] \
  [--transport acp|claude-stream|codex-app-server|opencode-serve] [--cancel-after s] [--startup-timeout s] \
  [--mode supervised|accept-edits|full-access] [--prompt texto] [--kind review|summary] [--related owner/repo]...
```

Sem `--transport` vale o padrão do agente. `--kind summary` roda um Summarize em vez de uma revisão, e cada `--related` acrescenta um repositório relacionado. `--mode` troca o modo de permissão da sessão e `--prompt` troca o prompt de revisão por um texto livre, útil para conferir que comandos e edições viram card. O exemplo nega todo pedido de permissão que chega a ele. Requer `gh` autenticado para o `github.com`.

Disco: os builds do workspace são grandes. Use `CARGO_INCREMENTAL=0` (`$env:CARGO_INCREMENTAL='0'` no PowerShell) e apague a pasta `target/` quando precisar liberar espaço.

## Próximos passos

Depois de um período de uso com os drivers nativos como padrão, o fallback ACP de Claude Code, Codex e OpenCode poderá ser removido; o ACP segue no Command Code.

## Créditos

Os formatos dos protocolos nativos foram adaptados de [albertofa/cw-code](https://github.com/albertofa/cw-code) (licença MIT).

## Instalação (Windows)

O instalador (`argon_<versão>_x64-setup.exe`) fica na página de Releases do repositório. Ele instala por usuário, sem pedir administrador, em `%LOCALAPPDATA%\Programs\Argon`, cria o atalho "Argon" no menu Iniciar e registra o desinstalador em "Aplicativos instalados". O atalho carrega o identificador `br.com.nuria.argon`, que é o que faz as notificações do Windows aparecerem como "Argon".

Os dados do usuário (`%APPDATA%\Argon` e `%LOCALAPPDATA%\Argon`) não são apagados ao desinstalar.

Nota de atualização: depois de abrir a versão com issues e Agent Plan, voltar para uma versão anterior (v0.1.5 ou antes) deixa o app antigo sem conseguir abrir o banco local. Ele mostra a inbox vazia até você atualizar de novo; nenhum dado se perde.

Gerar o instalador localmente:

```
cargo install cargo-packager --locked --version 0.11.8
cargo build -p argon-app --release
cargo packager --release --formats nsis
```

O instalador sai em `target/release/`. Para publicar uma versão, crie uma tag `v<versão>` igual à versão do `Cargo.toml`; o workflow `release.yml` gera o instalador e o anexa ao GitHub Release.
