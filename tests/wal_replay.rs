//! Logs a random request stream to the WAL while running it through one
//! engine, then replays the log into a fresh engine and requires identical
//! execution reports and final book state.

mod common;

use common::{Op, OpGenerator, TempFile};
use matching_engine::wal::{read, replay, WalRecord, WalWriter};
use matching_engine::{Engine, Side};

#[test]
#[cfg_attr(miri, ignore)] // Miri cannot map files.
fn replaying_the_wal_reproduces_every_event() {
    const OPS: u64 = 10_000;
    let path = TempFile::new("wal_replay");

    const CAPACITY: usize = 256;
    let mut primary = Engine::new(CAPACITY);
    let mut primary_events = Vec::new();
    // A small preallocation forces the writer to grow several times.
    let mut writer = WalWriter::create(&path.0, CAPACITY, 64).unwrap();
    let mut generator = OpGenerator::new(42);
    for sequence in 0..OPS {
        let record = match generator.next_op() {
            Op::Place(order) => WalRecord::submit(order, sequence),
            Op::Cancel { order_id } => WalRecord::cancel(order_id, sequence),
            Op::Reduce {
                order_id,
                reduce_by,
            } => WalRecord::reduce(order_id, reduce_by, sequence),
            Op::CancelAccount { account } => WalRecord::cancel_account(account, sequence),
            Op::CancelAll => WalRecord::cancel_all(sequence),
            Op::SetRiskLimits(_) => unreachable!("the generator sends no limit changes"),
        };
        // Write-ahead: the request is logged before the engine applies it.
        writer.append(record).unwrap();
        record.apply_to(&mut primary, &mut primary_events);
    }
    writer.finalize().unwrap();

    let contents = read(&path.0).unwrap();
    assert_eq!(contents.engine_capacity, Some(CAPACITY));
    let records = contents.records;
    assert_eq!(records.len() as u64, OPS);
    assert!(records.iter().zip(0..).all(|(r, seq)| r.sequence == seq));

    // The replica's ID map draws a different random hash seed; the results
    // must not depend on it.
    let mut replayed_events = Vec::new();
    let replica = replay(&path.0, &mut replayed_events).unwrap();

    assert_eq!(replayed_events, primary_events);
    assert_eq!(replica.live_order_count(), primary.live_order_count());
    for side in [Side::Buy, Side::Sell] {
        assert_eq!(
            replica.depth_snapshot(side, usize::MAX),
            primary.depth_snapshot(side, usize::MAX)
        );
    }
}
