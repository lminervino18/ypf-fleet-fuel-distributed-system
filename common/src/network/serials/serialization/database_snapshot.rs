use super::serialize_socket_address;
use crate::{
    network::serials::protocol::NO_LIMIT,
    operation::{AccountSnapshot, CardSnapshot, DatabaseSnapshot},
};

impl From<AccountSnapshot> for Vec<u8> {
    fn from(account: AccountSnapshot) -> Self {
        let account_id_srl = account.account_id.to_be_bytes();
        let limit_srl = account.limit.unwrap_or(NO_LIMIT).to_be_bytes();
        let consumed_srl = account.consumed.to_be_bytes();
        let mut srl = vec![];
        srl.extend(account_id_srl);
        srl.extend(limit_srl);
        srl.extend(consumed_srl);
        srl
    }
}

impl From<CardSnapshot> for Vec<u8> {
    fn from(card: CardSnapshot) -> Self {
        let account_id_srl = card.account_id.to_be_bytes();
        let card_id_srl = card.card_id.to_be_bytes();
        let limit_srl = card.limit.unwrap_or(NO_LIMIT).to_be_bytes();
        let consumed_srl = card.consumed.to_be_bytes();
        let mut srl = vec![];
        srl.extend(account_id_srl);
        srl.extend(card_id_srl);
        srl.extend(limit_srl);
        srl.extend(consumed_srl);
        srl
    }
}

impl From<DatabaseSnapshot> for Vec<u8> {
    fn from(database: DatabaseSnapshot) -> Self {
        let addr_srl = serialize_socket_address(database.addr);
        let accounts_srl_len = (database.accounts.len() as u16).to_be_bytes();
        let mut accounts_srl = vec![];
        for account in database.accounts {
            let account_srl: Vec<u8> = account.into();
            accounts_srl.extend(account_srl);
        }

        let cards_srl_len = (database.cards.len() as u16).to_be_bytes();
        let mut cards_srl = vec![];
        for card in database.cards {
            let card_srl: Vec<u8> = card.into();
            cards_srl.extend(card_srl);
        }

        let mut srl = vec![];
        srl.extend(addr_srl);
        srl.extend(accounts_srl_len);
        srl.extend(accounts_srl);
        srl.extend(cards_srl_len);
        srl.extend(cards_srl);
        srl
    }
}
