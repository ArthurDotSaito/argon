pub const KIND_COMPONENT: &str = "componente";
pub const KIND_SERVICE: &str = "serviço";
pub const KIND_QUEUE: &str = "fila";
pub const KIND_DATABASE: &str = "banco";
pub const KIND_CACHE: &str = "cache";
pub const KIND_EXTERNAL: &str = "externo";
pub const NOT_READ: &str = "não lido";
pub const LINK_CALLS: &str = "chama";
pub const LINK_SENDS_BY_QUEUE: &str = "envia por fila";
pub const LINK_TO: &str = "para";
pub const LINK_USES_AS_PACKAGE: &str = "usa como pacote";
pub const LINK_USES_PACKAGE: &str = "usa o pacote";
pub const LEGEND_SOLID: &str = "Contínua";
pub const LEGEND_SOLID_MEANING: &str = "chamada direta";
pub const LEGEND_DASHED: &str = "Tracejada";
pub const LEGEND_DASHED_MEANING: &str = "fila";
pub const LEGEND_UNDERLINED: &str = "Sublinhado";
pub const LEGEND_UNDERLINED_MEANING: &str = "projeto relacionado";
pub const EXCERPT_NOTE: &str = "Recorte: o que este PR toca e os vizinhos diretos";
pub const OPEN_BOARD: &str = "Abrir no quadro do projeto";

pub fn nodes_noun(count: usize) -> &'static str {
    if count == 1 { "nó" } else { "nós" }
}

pub fn edges_noun(count: usize) -> &'static str {
    if count == 1 {
        "ligação"
    } else {
        "ligações"
    }
}
