//! Logs a random request stream to the WAL while running it through one
//! engine, then replays the log into a fresh engine and requires identical
//! execution reports and final book state.

mod common;

use common::{Op, OpGenerator, TempFile};
use matching_engine::wal::{read_all, WalRecord, WalRecordKind, WalWriter};
use matching_engine::{Engine, Event, Side};

fn apply(engine: &mut Engine, record: &WalRecord, events: &mut Vec<Event>) {
    match record.kind {
        WalRecordKind::NewOrder => engine.process_order(
            record.order_id,
            record.side,
            record.price_tick,
            record.qty,
            events,
        ),
        WalRecordKind::Cancel => {
            engine.cancel_order(record.order_id, events);
        }
    }
}

#[test]
#[cfg_attr(miri, ignore)] // Miri cannot map files.
fn replaying_the_wal_reproduces_every_event() {
    const OPS: u64 = 10_000;
    let path = TempFile::new("wal_replay");

    let mut primary = Engine::new(256);
    let mut primary_events = Vec::new();
    // A small preallocation forces the writer to grow several times.
    let mut writer = WalWriter::create(&path.0, 64).unwrap();
    let mut generator = OpGenerator::new(42);
    for sequence in 0..OPS {
        let record = match generator.next_op() {
            // The log format has no order type yet: every order is logged,
            // and therefore applied, as a limit order.
            Op::Place {
                order_id,
                side,
                price_tick,
                qty,
                ..
            } => WalRecord::new_order(order_id, side, price_tick, qty, sequence),
            Op::Cancel { order_id } => WalRecord::cancel(order_id, sequence),
        };
        // Write-ahead: the request is logged before the engine applies it.
        writer.append(record).unwrap();
        apply(&mut primary, &record, &mut primary_events);
    }
    writer.finalize().unwrap();

    let records = read_all(&path.0).unwrap();
    assert_eq!(records.len() as u64, OPS);
    assert!(records.iter().zip(0..).all(|(r, seq)| r.sequence == seq));

    // The replica's ID map draws a different random hash seed; the results
    // must not depend on it.
    let mut replica = Engine::new(256);
    let mut replayed_events = Vec::new();
    for record in &records {
        apply(&mut replica, record, &mut replayed_events);
    }

    assert_eq!(replayed_events, primary_events);
    assert_eq!(replica.live_order_count(), primary.live_order_count());
    for side in [Side::Buy, Side::Sell] {
        assert_eq!(
            replica.depth_snapshot(side, usize::MAX),
            primary.depth_snapshot(side, usize::MAX)
        );
    }
}
