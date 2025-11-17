use actix::prelude::*;

/// Mensajes genéricos de alto nivel entre actores.
#[derive(Debug, Clone, Message)]
#[rtype(result = "()")]
pub enum ActorMsg {
    /// Mensaje de texto o debug
    Placeholder(String),

    /// Reenvío a tarjeta dentro de la cuenta
    CardMessage { card_id: u64, msg: Box<ActorMsg> },
}

/// Comandos de control del ActorRouter
#[derive(Debug, Clone, Message)]
#[rtype(result = "()")]
pub enum RouterCmd {
    /// Enviar mensaje a una cuenta
    SendToAccount { account_id: u64, msg: ActorMsg },

    /// Enviar mensaje a una tarjeta (dentro de una cuenta)
    SendToCard {
        account_id: u64,
        card_id: u64,
        msg: Box<ActorMsg>,
    },

    /// Listar las cuentas locales
    ListAccounts,
}
