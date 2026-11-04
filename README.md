# Argon

Aplicativo desktop nativo (Rust + GPUI Kit) que mostra os Pull Requests do GitHub como uma caixa de e-mail.

## Pré-requisitos

- Rust 1.96 (fixado em `rust-toolchain.toml`)
- Visual Studio Build Tools 2022 (MSVC)
- GitHub CLI (`gh`) autenticado (`gh auth login`)

## Executar

```
cargo run -p argon-app
```

## Agentes

O Argon abre uma sessão de revisão por agente de código instalado na máquina. Cada agente precisa do CLI correspondente instalado e autenticado; as versões abaixo são as validadas nesta branch.

| Agente | Protocolo | CLI exigido | Versão validada |
| --- | --- | --- | --- |
| Claude Code | nativo (`claude -p` com stream-json, padrão) ou ACP | `claude` | 2.1.316 |
| Codex | nativo (`codex app-server`) ou ACP | `codex` | codex-cli 0.157.0 |
| OpenCode | nativo (`opencode serve` privado, padrão) ou ACP | `opencode` | 2.2.5 |
| Command Code | ACP | `command-code` | via ACP |

- **Nativo**: o Argon fala direto com o CLI. **ACP**: o Argon fala com um adapter ACP, que o `npx` baixa na hora (Claude Code e Codex) ou que já vem no CLI (OpenCode e Command Code). O ACP continua como único caminho do Command Code e como fallback por agente, na opção "Usar adapter ACP (legado)" das configurações.
- OpenCode e Claude Code já rodam no driver nativo por padrão. O Codex continua em ACP até concluir a validação ponta a ponta; o driver nativo dele pode ser exercitado pelo exemplo `run_session` (veja "Notas para desenvolvedores").
- No Claude Code nativo, se o processo do CLI sair entre um turno e outro, a sessão continua: a próxima mensagem retoma a conversa com `--resume`.

### Segurança

A configuração de agente que vem dentro do PR revisado não é confiável. Cada driver nativo a ignora:

- **Claude Code**: `--setting-sources user --strict-mcp-config --permission-mode default`, sem settings nem MCP do projeto.
- **OpenCode**: `OPENCODE_CONFIG_DIR` apontando para uma pasta do Argon e `OPENCODE_DISABLE_PROJECT_CONFIG` ligado. O Argon mantém nessa pasta um `opencode.json` com `"permission": {"*": "ask"}`, então toda ferramenta pede permissão ao Argon. A mesma política vai também em `OPENCODE_CONFIG_CONTENT`, que tem a maior precedência, e o Argon sempre sobrescreve essa variável, mesmo que o env do agente a defina.
- **Codex**: o diretório da revisão entra em cada thread com `trust_level = "untrusted"` e `approvalsReviewer: "user"`, então as aprovações que o Codex pede chegam ao Argon.

O que cada driver nativo pede ao Argon:

- **Claude Code e OpenCode**: comandos, edições e buscas na web viram pedido de permissão para o Argon. No OpenCode, negar um card encerra o turno; para continuar, mande outra mensagem ao agente. No Claude Code, as regras `permissions.allow` do próprio usuário em `~/.claude/settings.json` continuam valendo nas revisões do Argon: o que elas liberam roda sem card.
- **Codex**: em Supervisionado e Aceitar edições, todo comando do Codex vira card, inclusive leituras como `git diff` e `rg`, porque a sandbox somente leitura do Codex no Windows não impede ler fora da worktree. "Permitir nesta sessão" reduz a repetição, mas só aparece em comandos que o Codex classifica inteiramente como leitura, listagem ou busca; o Codex guarda essa aprovação em memória, só na sessão aberta. Comandos que podem alterar algo (como `npm test`), escritas no stdin de um processo e pedidos de rede ou de permissões extras sempre pedem card. O card mostra a pasta do comando quando ela não é a worktree da revisão. Nesses dois modos a busca na web do Codex fica desligada (`web_search = "disabled"`). Em Acesso total os comandos rodam sem card.
- Os títulos dos cards passam por um único filtro: caracteres de controle, bidi e de largura zero viram espaço, e o texto é cortado em 2000 caracteres (comandos, rede e outros) ou 300 (leituras, buscas e edições), sempre com " …" visível quando há corte.

Modos de permissão, por agente:

- **Supervisionado**: leituras e buscas dentro da worktree da revisão são aprovadas sem card; o resto que o agente pede ao Argon aparece em um card para aprovar ou negar.
- **Aceitar edições**: leituras, buscas e edições dentro da worktree da revisão são aprovadas automaticamente; o resto continua pedindo.
- **Acesso total**: aprova tudo sem mostrar o card. O agente lê código de PR de terceiros, então use só se aceitar o risco. No Codex exige a sandbox do Windows pronta; sem ela a sessão abre em Supervisionado. Sair do Acesso total no meio de um turno do Codex interrompe o turno e encerra o processo do Codex, com os processos em segundo plano que ele abriu; a próxima mensagem retoma a conversa já no modo mais restrito.

"Permitir nesta sessão" vale só para a sessão aberta, fica em memória (não é gravado em disco) e só vale para leituras, buscas e edições dentro da worktree da revisão. A exceção são os comandos do Codex, descritos acima.

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
  [--mode supervised|accept-edits|full-access] [--prompt texto]
```

Sem `--transport` vale o padrão do agente. `--mode` troca o modo de permissão da sessão e `--prompt` troca o prompt de revisão por um texto livre, útil para conferir que comandos e edições viram card. O exemplo nega todo pedido de permissão que chega a ele. Requer `gh` autenticado para o `github.com`.

Disco: os builds do workspace são grandes. Use `CARGO_INCREMENTAL=0` (`$env:CARGO_INCREMENTAL='0'` no PowerShell) e apague a pasta `target/` quando precisar liberar espaço.

## Próximos passos

Os drivers nativos viram o padrão de Claude Code, Codex e OpenCode depois da validação ponta a ponta com os CLIs reais. Só então o fallback ACP desses três poderá ser removido; o ACP segue no Command Code.

## Créditos

Os formatos dos protocolos nativos foram adaptados de [albertofa/cw-code](https://github.com/albertofa/cw-code) (licença MIT).

## Instalação (Windows)

O instalador (`argon_<versão>_x64-setup.exe`) fica na página de Releases do repositório. Ele instala por usuário, sem pedir administrador, em `%LOCALAPPDATA%\Programs\Argon`, cria o atalho "Argon" no menu Iniciar e registra o desinstalador em "Aplicativos instalados". O atalho carrega o identificador `br.com.nuria.argon`, que é o que faz as notificações do Windows aparecerem como "Argon".

Os dados do usuário (`%APPDATA%\Argon` e `%LOCALAPPDATA%\Argon`) não são apagados ao desinstalar.

Gerar o instalador localmente:

```
cargo install cargo-packager --locked --version 0.11.8
cargo build -p argon-app --release
cargo packager --release --formats nsis
```

O instalador sai em `target/release/`. Para publicar uma versão, crie uma tag `v<versão>` igual à versão do `Cargo.toml`; o workflow `release.yml` gera o instalador e o anexa ao GitHub Release.
