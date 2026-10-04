//! Recovery from a checkpoint and the end of the log (chapter 9).
//!
//! A live engine logs every request before applying it, writes a checkpoint
//! half-way through, and keeps going. Then recovery rebuilds the book the
//! fast way: restore the checkpoint, and replay only the requests logged
//! after it. The result must be the live engine's book.
//!
//! Run it with `cargo run --example checkpoint`.

use matching_engine::snapshot::{read_checkpoint, write_checkpoint};
use matching_engine::wal::{replay_from, WalRecord, WalWriter};
use matching_engine::{Checkpoint, Engine, Event, NewOrder, Side};

/// Request number `sequence` of a simple, repeatable stream: orders around
/// tick 1000, and every fifth request cancels the order before it.
fn request(sequence: u64) -> WalRecord {
    let id = sequence + 1;
    if id.is_multiple_of(5) {
        return WalRecord::cancel(id - 1, sequence);
    }
    let side = if id.is_multiple_of(2) {
        Side::Buy
    } else {
        Side::Sell
    };
    let order = NewOrder::limit(id, side, 1_000 + (id % 7) as u32, 1 + id % 3);
    WalRecord::submit(order, sequence)
}

fn main() -> std::io::Result<()> {
    const N: u64 = 10_000;
    const CAPACITY: usize = 50_000;
    let dir = std::env::temp_dir();
    let log_path = dir.join("matching_engine_checkpoint.wal");
    let checkpoint_path = dir.join("matching_engine_checkpoint.snap");
    for path in [&log_path, &checkpoint_path] {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
    }

    // The live run. Its events would go downstream; here they are dropped.
    let mut ignore = |_: Event| {};
    let mut engine = Engine::new(CAPACITY);
    let mut wal = WalWriter::create(&log_path, CAPACITY, 16_384)?;
    for sequence in 0..N {
        let record = request(sequence);
        wal.append(record)?; // write-ahead: log first...
        record.apply_to(&mut engine, &mut ignore); // ...then apply
        if sequence == N / 2 {
            // The snapshot includes every request up to this one, so
            // recovery must replay from the next.
            let checkpoint = Checkpoint {
                next_sequence: sequence + 1,
                snapshot: engine.snapshot(),
            };
            write_checkpoint(&checkpoint_path, &checkpoint)?;
        }
    }
    wal.finalize()?;

    // Recovery: the checkpoint, then the end of the log.
    let checkpoint = read_checkpoint(&checkpoint_path)?;
    let mut recovered = Engine::restore(&checkpoint.snapshot)?;
    let replayed = replay_from(
        &log_path,
        &mut recovered,
        checkpoint.next_sequence,
        &mut ignore,
    )?;
    for path in [&log_path, &checkpoint_path] {
        std::fs::remove_file(path)?;
    }

    assert_eq!(recovered.snapshot(), engine.snapshot());
    println!(
        "restored {} resting orders from the checkpoint and replayed {replayed} of {N} \
         requests: identical book",
        checkpoint.snapshot.orders.len()
    );
    Ok(())
}
