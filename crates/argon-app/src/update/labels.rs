pub const IDLE: &str = "Ainda não verificado";
pub const CHECKING: &str = "Verificando atualizações…";
pub const UP_TO_DATE: &str = "Você está na versão mais recente";
pub const INSTALLING: &str = "Abrindo o instalador…";
pub const NO_RELEASE: &str = "Nenhuma release encontrada ou sem acesso ao repositório";
pub const AUTH_MISSING: &str = "Sem acesso ao GitHub — rode `gh auth login` e verifique de novo";
pub const FORBIDDEN: &str = "Sem permissão para ler as releases do repositório";
pub const NO_INSTALLER: &str = "A release não tem um instalador para Windows";
pub const NO_CHECKSUM: &str =
    "A release não tem checksum do instalador; atualize manualmente pelo GitHub";
pub const TOAST_HINT: &str = "Abra Configurações › Sobre para atualizar.";

pub fn available(version: &str) -> String {
    format!("Nova versão disponível: v{version}")
}

pub fn downloading(percent: u8) -> String {
    format!("Baixando {percent}%")
}

pub fn badge_available(version: &str) -> String {
    format!("v{version} disponível")
}

pub fn check_failed(error: &str) -> String {
    format!("Falha ao verificar atualizações: {error}")
}

pub fn install_failed(error: &str) -> String {
    format!("Falha ao atualizar: {error}")
}

pub fn toast_body(version: &str) -> String {
    format!("Argon v{version} disponível. {TOAST_HINT}")
}

pub fn invalid_version(tag: &str) -> String {
    format!("Release com versão inválida: {tag}")
}
