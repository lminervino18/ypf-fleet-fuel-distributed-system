//! Serialization of operation results.
//!
//! Implement `From<OperationResult> for Vec<u8>` and provide helper serializers
//! for each concrete result type. Each serialized result begins with a type
//! byte followed by type-specific payload. The layout mirrors the
//! deserialization logic used by the receiving side.

use crate::network::serials::protocol::*;
use crate::operation::DatabaseSnapshot;
use crate::operation_result::OperationResult::*;
use crate::operation_result::*;

/// Serialize an `OperationResult` into a byte vector.
impl From<OperationResult> for Vec<u8> {
    fn from(op_result: OperationResult) -> Self {
        match op_result {
            Charge(result) => serialize_charge_result(result),
            LimitAccount(result) => serialize_limit_account_result(result),
            LimitCard(result) => serialize_limit_card_result(result),
            AccountQuery(result) => serialize_account_query_result(result),
            DatabaseSnapshot(result) => serialize_database_snapshot_result(result),
            ReplaceDatabase => serialize_replace_database_result(),
        }
    }
}

/// Serialize a ReplaceDatabase result (no payload).
fn serialize_replace_database_result() -> Vec<u8> {
    let type_srl = REPLACE_DATABASE;
    let srl = vec![type_srl];
    srl
}

/// Serialize a DatabaseSnapshot result.
///
/// The snapshot is appended after the type byte.
fn serialize_database_snapshot_result(result: DatabaseSnapshot) -> Vec<u8> {
    let type_srl = DATABASE_SNAPSHOT;
    let database_srl: Vec<u8> = result.into();
    let mut srl = vec![];
    srl.push(type_srl);
    srl.extend(database_srl);
    srl
}

/// Serialize a single (card_id, spent) entry used in account query results.
fn serialize_card_spent(card_spent: (u64, f32)) -> Vec<u8> {
    let card_id_srl = card_spent.0.to_be_bytes();
    let spent_srl = card_spent.1.to_be_bytes();
    let mut srl = vec![];
    srl.extend(card_id_srl);
    srl.extend(spent_srl);
    srl
}

/// Serialize an AccountQuery result.
///
/// Layout:
/// - type byte (ACC_QUERY_RESULT)
/// - account_id (u64)
/// - total_spent (f32)
/// - per_card_spent_len (usize, native endian)
/// - sequence of (card_id, spent) entries
fn serialize_account_query_result(result: AccountQueryResult) -> Vec<u8> {
    let type_srl = [ACC_QUERY_RESULT];
    let account_id_srl = result.account_id.to_be_bytes();
    let total_spent_srl = result.total_spent.to_be_bytes();
    let per_card_spent_len_srl = result.per_card_spent.len().to_be_bytes();
    let mut srl = vec![];
    srl.extend(type_srl);
    srl.extend(account_id_srl);
    srl.extend(total_spent_srl);
    srl.extend(per_card_spent_len_srl);
    for card_spent in result.per_card_spent {
        srl.extend(serialize_card_spent(card_spent));
    }

    srl
}

/// Serialize a LimitCard result: type byte + serialized LimitResult.
fn serialize_limit_card_result(result: LimitResult) -> Vec<u8> {
    let type_srl = LIMIT_CARD_RESULT;
    let mut srl = vec![];
    srl.push(type_srl);
    srl.extend(serialize_limit_result(result));
    srl
}

/// Serialize a LimitAccount result: type byte + serialized LimitResult.
fn serialize_limit_account_result(result: LimitResult) -> Vec<u8> {
    let type_srl = LIMIT_ACCOUNT_RESULT;
    let mut srl = vec![];
    srl.push(type_srl);
    srl.extend(serialize_limit_result(result));
    srl
}

/// Serialize a Charge result.
///
/// On success the payload is the OK byte; on failure the payload is ERR
/// followed by the serialized VerifyError.
fn serialize_charge_result(result: ChargeResult) -> Vec<u8> {
    let type_srl = CHARGE_RESULT;
    let mut srl = vec![];
    srl.push(type_srl);
    match result {
        ChargeResult::Ok => srl.push(OK),
        ChargeResult::Failed(error) => {
            srl.push(ERR);
            let error_srl: Vec<u8> = error.into();
            srl.extend(error_srl);
        }
    }

    srl
}

/// Serialize a LimitResult.
///
/// Returns a vector beginning with OK or ERR. If ERR follows with the error payload.
fn serialize_limit_result(result: LimitResult) -> Vec<u8> {
    let mut limit_result_srl = vec![];
    match result {
        LimitResult::Ok => limit_result_srl.push(OK),
        LimitResult::Failed(error) => {
            limit_result_srl.push(ERR);
            let error_srl: Vec<u8> = error.into();
            limit_result_srl.extend(error_srl);
        }
    }

    limit_result_srl
}
