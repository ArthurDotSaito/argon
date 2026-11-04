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
| Claude Code | nativo (`claude -p` com stream-json) ou ACP | `claude` | 2.1.316 |
| Codex | nativo (`codex app-server`) ou ACP | `codex` | codex-cli 0.157.0 |
| OpenCode | nativo (`opencode serve` privado) ou ACP | `opencode` | 2.2.5 |
| Command Code | ACP | `command-code` | via ACP |

- **Nativo**: o Argon fala direto com o CLI. **ACP**: o Argon fala com um adapter ACP, que o `npx` baixa na hora (Claude Code e Codex) ou que já vem no CLI (OpenCode e Command Code). O ACP continua como único caminho do Command Code e como fallback por agente, na opção "Usar adapter ACP (legado)" das configurações.
- Enquanto a validação ponta a ponta não termina, o lançamento padrão de Claude Code, Codex e OpenCode ainda é ACP. Os drivers nativos são opt-in pelo exemplo `run_session` (veja "Notas para desenvolvedores").

### Segurança

A configuração de agente que vem dentro do PR revisado não é confiável. Cada driver nativo a ignora:

- **Claude Code**: `--setting-sources user --strict-mcp-config`, sem settings nem MCP do projeto.
- **OpenCode**: `OPENCODE_CONFIG_DIR` apontando para uma pasta do Argon e `OPENCODE_DISABLE_PROJECT_CONFIG` ligado.
- **Codex**: o diretório da revisão entra em cada thread com `trust_level = "untrusted"` e `approvalsReviewer: "user"`, então as aprovações sempre chegam ao Argon.

Modos de permissão, por agente:

- **Supervisionado**: todo pedido de permissão aparece em um card para aprovar ou negar.
- **Aceitar edições**: leituras, buscas e edições dentro da worktree da revisão são aprovadas automaticamente; o resto continua pedindo.
- **Acesso total**: aprova tudo sem mostrar o card. O agente lê código de PR de terceiros, então use só se aceitar o risco. No Codex exige a sandbox do Windows pronta; sem ela a sessão abre em Supervisionado.

"Permitir nesta sessão" vale só para a sessão aberta, fica em memória (não é gravado em disco) e só vale para leituras, buscas e edições dentro da worktree da revisão.

### Uso do plano

Os cartões de agente mostram quanto do plano já foi usado:

- **Claude Code e Codex**: lido pelo CLI instalado, sem gastar tokens.
- **OpenCode Go**: só é consultado com consentimento explícito do usuário.
- **Command Code**: mostra um link para a página de uso (https://commandcode.ai/usage).

## Notas para desenvolvedores

`run_session` roda uma sessão de revisão completa contra um PR real, pela linha de comando, e permite escolher o transporte:

```
cargo run -p argon-agents --example run_session -- <claude|codex|opencode|command-code> <owner/repo#n> [modelo] \
  [--transport acp|claude-stream|codex-app-server|opencode-serve] [--cancel-after s] [--startup-timeout s]
```

Sem `--transport` vale o padrão atual (ACP). Para exercitar os drivers nativos use `claude-stream`, `codex-app-server` ou `opencode-serve`. Requer `gh` autenticado para o `github.com`.

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
