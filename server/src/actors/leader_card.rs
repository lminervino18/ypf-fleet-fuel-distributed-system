use actix::prelude::*;
use super::actor_router::ActorRouter;
use super::types::ActorMsg;
use log::info;

/// LeaderCard actor: unique per card_id (one leader cluster-wide).
pub struct LeaderCard {
    pub card_id: u64,
    pub actor_router: Addr<ActorRouter>,
}

impl LeaderCard {
    pub fn new(card_id: u64, actor_router: Addr<ActorRouter>) -> Self {
        Self { card_id, actor_router }
    }
}

impl Actor for LeaderCard {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("[LeaderCard {}] started", self.card_id);
    }
}

/// Minimal handler so router can `do_send(ActorMsg)` to this actor.
impl Handler<ActorMsg> for LeaderCard {
    type Result = ();

    fn handle(&mut self, msg: ActorMsg, _ctx: &mut Context<Self>) {
        info!("[LeaderCard {}] received {:?}", self.card_id, msg);
        // TODO: your real business logic here
    }
}
