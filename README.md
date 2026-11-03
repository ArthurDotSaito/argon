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
