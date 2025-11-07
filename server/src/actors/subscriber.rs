use actix::prelude::*;
use super::actor_router::ActorRouter;
use super::types::ActorMsg;
use log::info;

/// Subscriber actor: local replica for (card_id) on this node.
/// NOTE: many nodes host Subscriber for the same card_id.
/// Global addressing uses (node_id, card_id); locally we index only by card_id.
pub struct Subscriber {
    pub card_id: u64,
    pub actor_router: Addr<ActorRouter>,
}

impl Subscriber {
    pub fn new(card_id: u64, actor_router: Addr<ActorRouter>) -> Self {
        Self { card_id, actor_router }
    }
}

impl Actor for Subscriber {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("[Subscriber card={}] started", self.card_id);
    }
}

/// Minimal handler so router can `do_send(ActorMsg)` to this actor.
impl Handler<ActorMsg> for Subscriber {
    type Result = ();

    fn handle(&mut self, msg: ActorMsg, _ctx: &mut Context<Self>) {
        info!("[Subscriber card={}] received {:?}", self.card_id, msg);
        // TODO: your real business logic here
    }
}
