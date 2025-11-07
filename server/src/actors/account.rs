use actix::prelude::*;
use super::actor_router::ActorRouter;
use super::types::ActorMsg;
use log::info;

/// Account actor: unique per account_id.
pub struct Account {
    pub account_id: u64,
    pub actor_router: Addr<ActorRouter>,
}

impl Account {
    pub fn new(account_id: u64, actor_router: Addr<ActorRouter>) -> Self {
        Self { account_id, actor_router }
    }
}

impl Actor for Account {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("[Account {}] started", self.account_id);
    }
}

/// Minimal handler so router can `do_send(ActorMsg)` to this actor.
impl Handler<ActorMsg> for Account {
    type Result = ();

    fn handle(&mut self, msg: ActorMsg, _ctx: &mut Context<Self>) {
        info!("[Account {}] received {:?}", self.account_id, msg);
        // TODO: your real business logic here
    }
}
